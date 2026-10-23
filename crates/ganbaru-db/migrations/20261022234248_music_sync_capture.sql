-- Music replicates through the vault manifest. Tables with a composite primary key gain the
-- manifest row key `sync_key`; existing rows hold a `pending:` placeholder that the Music
-- conversion pass replaces with the derived key at vault open. Constraints the engine cannot
-- resolve move into rules it can: a source collection's identity key determines its root or
-- playlist, skip range order is checked by the Music adapter, and generated noise soundscapes
-- keep their fixed seed keys through local triggers.

-- Composite key tables, rebuilt with `sync_key`.
CREATE TABLE music_item_signals_next (
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    signal TEXT NOT NULL
        CHECK (signal IN ('lyrics', 'sudden-changes', 'high-intensity', 'calm', 'repetitive', 'energizing')),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    sync_key TEXT NOT NULL UNIQUE,
    PRIMARY KEY (item_id, signal)
);
INSERT INTO music_item_signals_next (item_id, signal, created_at_ms, sync_key)
SELECT item_id, signal, created_at_ms, 'pending:' || rowid FROM music_item_signals;
DROP TABLE music_item_signals;
ALTER TABLE music_item_signals_next RENAME TO music_item_signals;

CREATE TABLE music_listening_statistics_next (
    device_id TEXT NOT NULL CHECK (trim(device_id) <> ''),
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    last_played_at_ms INTEGER,
    play_count INTEGER NOT NULL DEFAULT 0 CHECK (play_count >= 0),
    completion_count INTEGER NOT NULL DEFAULT 0 CHECK (completion_count >= 0),
    skip_count INTEGER NOT NULL DEFAULT 0 CHECK (skip_count >= 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    sync_key TEXT NOT NULL UNIQUE,
    PRIMARY KEY (device_id, item_id)
);
INSERT INTO music_listening_statistics_next (
    device_id, item_id, last_played_at_ms, play_count, completion_count, skip_count,
    updated_at_ms, sync_key
)
SELECT device_id, item_id, last_played_at_ms, play_count, completion_count, skip_count,
    updated_at_ms, 'pending:' || rowid
FROM music_listening_statistics;
DROP INDEX idx_music_listening_statistics_item;
DROP TABLE music_listening_statistics;
ALTER TABLE music_listening_statistics_next RENAME TO music_listening_statistics;
CREATE INDEX idx_music_listening_statistics_item ON music_listening_statistics(item_id);

CREATE TABLE music_playlist_intended_uses_next (
    playlist_id TEXT NOT NULL REFERENCES music_playlists(id) ON DELETE CASCADE,
    intended_use TEXT NOT NULL
        CHECK (intended_use IN ('general', 'focus', 'reading', 'relaxation', 'energizing')),
    sync_key TEXT NOT NULL UNIQUE,
    PRIMARY KEY (playlist_id, intended_use)
);
INSERT INTO music_playlist_intended_uses_next (playlist_id, intended_use, sync_key)
SELECT playlist_id, intended_use, 'pending:' || rowid FROM music_playlist_intended_uses;
DROP TABLE music_playlist_intended_uses;
ALTER TABLE music_playlist_intended_uses_next RENAME TO music_playlist_intended_uses;

CREATE TABLE music_soundscape_preset_sounds_next (
    preset_id TEXT NOT NULL REFERENCES music_soundscape_presets(id) ON DELETE CASCADE,
    soundscape_id TEXT NOT NULL REFERENCES music_soundscapes(id) ON DELETE CASCADE,
    order_key TEXT NOT NULL CHECK (length(order_key) BETWEEN 1 AND 128),
    level_milli INTEGER NOT NULL DEFAULT 1000 CHECK (level_milli BETWEEN 0 AND 1000),
    sync_key TEXT NOT NULL UNIQUE,
    PRIMARY KEY (preset_id, soundscape_id)
);
INSERT INTO music_soundscape_preset_sounds_next (
    preset_id, soundscape_id, order_key, level_milli, sync_key
)
SELECT preset_id, soundscape_id, order_key, level_milli, 'pending:' || rowid
FROM music_soundscape_preset_sounds;
DROP INDEX idx_music_soundscape_preset_sounds_sound;
DROP TABLE music_soundscape_preset_sounds;
ALTER TABLE music_soundscape_preset_sounds_next RENAME TO music_soundscape_preset_sounds;
CREATE INDEX idx_music_soundscape_preset_sounds_sound
    ON music_soundscape_preset_sounds(soundscape_id);

-- The seeded preset sounds take their derived keys here, `derived_key("music-preset-sound",
-- [preset_id, soundscape_id])`, so a fresh vault holds no placeholder.
UPDATE music_soundscape_preset_sounds
SET sync_key = 'music-preset-sound:92d6ae2884258f01f7a9208ca6e6cb93'
WHERE preset_id = 'preset-generated-white-noise' AND soundscape_id = 'generated-white-noise';
UPDATE music_soundscape_preset_sounds
SET sync_key = 'music-preset-sound:5a45c7668bf66c2f8e55a7bae3d8002b'
WHERE preset_id = 'preset-generated-pink-noise' AND soundscape_id = 'generated-pink-noise';
UPDATE music_soundscape_preset_sounds
SET sync_key = 'music-preset-sound:631e8842bea702f6c51630904f31cc9e'
WHERE preset_id = 'preset-generated-brown-noise' AND soundscape_id = 'generated-brown-noise';

CREATE TABLE music_context_assignments_next (
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
    sync_key TEXT NOT NULL UNIQUE,
    PRIMARY KEY (owner_kind, owner_id, phase),
    CHECK (playlist_id IS NULL OR trim(playlist_id) <> ''),
    CHECK (soundscape_preset_id IS NULL OR trim(soundscape_preset_id) <> ''),
    CHECK (provenance_id IS NULL OR trim(provenance_id) <> '')
);
INSERT INTO music_context_assignments_next (
    owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind, provenance_id,
    updated_at_ms, version, soundscape_behavior, soundscape_preset_id, sync_key
)
SELECT owner_kind, owner_id, phase, behavior, playlist_id, provenance_kind, provenance_id,
    updated_at_ms, version, soundscape_behavior, soundscape_preset_id, 'pending:' || rowid
FROM music_context_assignments;
DROP INDEX idx_music_context_assignments_owner;
DROP INDEX idx_music_context_assignments_playlist;
DROP TABLE music_context_assignments;
ALTER TABLE music_context_assignments_next RENAME TO music_context_assignments;
CREATE INDEX idx_music_context_assignments_owner
    ON music_context_assignments(owner_kind, owner_id, phase);
CREATE INDEX idx_music_context_assignments_playlist
    ON music_context_assignments(playlist_id, owner_kind, owner_id);

-- Skip ranges are an owned list the adapter checks as a whole: each range ends after it starts
-- and no range starts before the previous one ends.
CREATE TABLE music_membership_skip_ranges_next (
    membership_id TEXT NOT NULL REFERENCES music_playlist_memberships(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal BETWEEN 0 AND 63),
    start_ms INTEGER NOT NULL CHECK (start_ms >= 0),
    end_ms INTEGER NOT NULL CHECK (end_ms >= 0),
    PRIMARY KEY (membership_id, ordinal)
);
INSERT INTO music_membership_skip_ranges_next (membership_id, ordinal, start_ms, end_ms)
SELECT membership_id, ordinal, start_ms, end_ms FROM music_membership_skip_ranges;
DROP TABLE music_membership_skip_ranges;
ALTER TABLE music_membership_skip_ranges_next RENAME TO music_membership_skip_ranges;

-- Source collections: the identity key names the root or playlist, so one collection per root
-- follows from the unique identity key. Dropping the table deletes its children through their
-- foreign keys, so the children are copied aside and restored after the rebuild.
CREATE TEMP TABLE music_refresh_jobs_copy AS SELECT * FROM music_refresh_jobs;
CREATE TEMP TABLE music_refresh_job_entries_copy AS SELECT * FROM music_refresh_job_entries;
CREATE TEMP TABLE music_refresh_job_issues_copy AS SELECT * FROM music_refresh_job_issues;
CREATE TEMP TABLE music_source_collection_items_copy AS
SELECT rowid AS source_rowid, * FROM music_source_collection_items;

DROP INDEX idx_music_refresh_job_entries_pending;
DROP TABLE music_refresh_job_entries;
DROP INDEX idx_music_refresh_job_issues_job;
DROP TABLE music_refresh_job_issues;
DROP INDEX idx_music_refresh_jobs_source_state;
DROP TABLE music_refresh_jobs;
DROP INDEX idx_music_source_collection_items_item;
DROP INDEX idx_music_source_collection_items_order;
DROP TABLE music_source_collection_items;

CREATE TABLE music_source_collections_next (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('local-root', 'youtube-playlist')),
    identity_key TEXT NOT NULL UNIQUE CHECK (trim(identity_key) <> ''),
    name TEXT NOT NULL CHECK (trim(name) <> ''),
    local_root_id TEXT REFERENCES music_local_roots(id) ON DELETE CASCADE,
    youtube_playlist_id TEXT UNIQUE,
    refresh_state TEXT NOT NULL DEFAULT 'idle'
        CHECK (refresh_state IN ('idle', 'queued', 'running', 'partial', 'failed')),
    last_successful_refresh_at_ms INTEGER,
    last_refresh_error_code TEXT,
    snapshot_generation INTEGER NOT NULL DEFAULT 0 CHECK (snapshot_generation >= 0),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0),
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    version INTEGER NOT NULL DEFAULT 1 CHECK (version > 0),
    discovery_enabled INTEGER NOT NULL DEFAULT 1
        CHECK (discovery_enabled IN (0, 1)),
    removed_at_ms INTEGER,
    previous_successful_refresh_at_ms INTEGER,
    CHECK (
        (
            kind = 'local-root'
            AND local_root_id IS NOT NULL
            AND youtube_playlist_id IS NULL
            AND identity_key = 'local-root:' || local_root_id
        )
        OR (
            kind = 'youtube-playlist'
            AND local_root_id IS NULL
            AND youtube_playlist_id IS NOT NULL
            AND trim(youtube_playlist_id) <> ''
            AND identity_key = 'youtube-playlist:' || youtube_playlist_id
        )
    )
);
INSERT INTO music_source_collections_next (
    id, kind, identity_key, name, local_root_id, youtube_playlist_id, refresh_state,
    last_successful_refresh_at_ms, last_refresh_error_code, snapshot_generation, created_at_ms,
    updated_at_ms, version, discovery_enabled, removed_at_ms, previous_successful_refresh_at_ms
)
SELECT id, kind, identity_key, name, local_root_id, youtube_playlist_id, refresh_state,
    last_successful_refresh_at_ms, last_refresh_error_code, snapshot_generation, created_at_ms,
    updated_at_ms, version, discovery_enabled, removed_at_ms, previous_successful_refresh_at_ms
FROM music_source_collections;
DROP INDEX idx_music_source_collections_kind_state;
DROP TABLE music_source_collections;
ALTER TABLE music_source_collections_next RENAME TO music_source_collections;
CREATE INDEX idx_music_source_collections_kind_state
    ON music_source_collections(kind, refresh_state, updated_at_ms);

CREATE TABLE music_source_collection_items (
    collection_id TEXT NOT NULL REFERENCES music_source_collections(id) ON DELETE CASCADE,
    item_id TEXT NOT NULL REFERENCES music_library_items(id) ON DELETE CASCADE,
    source_position INTEGER CHECK (source_position IS NULL OR source_position >= 0),
    first_discovered_at_ms INTEGER NOT NULL CHECK (first_discovered_at_ms > 0),
    last_seen_generation INTEGER NOT NULL DEFAULT 0 CHECK (last_seen_generation >= 0),
    missing_from_latest_snapshot INTEGER NOT NULL DEFAULT 0
        CHECK (missing_from_latest_snapshot IN (0, 1)),
    sync_key TEXT NOT NULL UNIQUE,
    PRIMARY KEY (collection_id, item_id)
);
INSERT INTO music_source_collection_items (
    collection_id, item_id, source_position, first_discovered_at_ms, last_seen_generation,
    missing_from_latest_snapshot, sync_key
)
SELECT collection_id, item_id, source_position, first_discovered_at_ms, last_seen_generation,
    missing_from_latest_snapshot, 'pending:' || source_rowid
FROM temp.music_source_collection_items_copy;
CREATE INDEX idx_music_source_collection_items_item
    ON music_source_collection_items(item_id, collection_id);
CREATE INDEX idx_music_source_collection_items_order
    ON music_source_collection_items(collection_id, source_position, item_id);

CREATE TABLE music_refresh_jobs (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    source_collection_id TEXT NOT NULL REFERENCES music_source_collections(id) ON DELETE CASCADE,
    local_root_id TEXT REFERENCES music_local_roots(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('local-root', 'youtube-playlist')),
    state TEXT NOT NULL CHECK (
        state IN ('queued', 'running', 'completed', 'partial', 'failed', 'cancelled')
    ),
    generation INTEGER NOT NULL CHECK (generation > 0),
    discovered_count INTEGER NOT NULL DEFAULT 0 CHECK (discovered_count >= 0),
    processed_count INTEGER NOT NULL DEFAULT 0 CHECK (processed_count >= 0),
    skipped_count INTEGER NOT NULL DEFAULT 0 CHECK (skipped_count >= 0),
    issue_count INTEGER NOT NULL DEFAULT 0 CHECK (issue_count >= 0),
    truncated_count INTEGER NOT NULL DEFAULT 0 CHECK (truncated_count >= 0),
    absence_determined INTEGER NOT NULL DEFAULT 0 CHECK (absence_determined IN (0, 1)),
    status_message TEXT NOT NULL DEFAULT '',
    requested_at_ms INTEGER NOT NULL CHECK (requested_at_ms > 0),
    started_at_ms INTEGER,
    finished_at_ms INTEGER,
    updated_at_ms INTEGER NOT NULL CHECK (updated_at_ms > 0),
    CHECK (
        (kind = 'local-root' AND local_root_id IS NOT NULL)
        OR (kind = 'youtube-playlist' AND local_root_id IS NULL)
    )
);
INSERT INTO music_refresh_jobs (
    id, source_collection_id, local_root_id, kind, state, generation, discovered_count,
    processed_count, skipped_count, issue_count, truncated_count, absence_determined,
    status_message, requested_at_ms, started_at_ms, finished_at_ms, updated_at_ms
)
SELECT id, source_collection_id, local_root_id, kind, state, generation, discovered_count,
    processed_count, skipped_count, issue_count, truncated_count, absence_determined,
    status_message, requested_at_ms, started_at_ms, finished_at_ms, updated_at_ms
FROM temp.music_refresh_jobs_copy;
CREATE INDEX idx_music_refresh_jobs_source_state
    ON music_refresh_jobs(source_collection_id, state, generation DESC);

CREATE TABLE music_refresh_job_entries (
    job_id TEXT NOT NULL REFERENCES music_refresh_jobs(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    entry_kind TEXT NOT NULL CHECK (entry_kind IN ('directory', 'media')),
    state TEXT NOT NULL DEFAULT 'pending' CHECK (state IN ('pending', 'processed', 'skipped')),
    PRIMARY KEY (job_id, relative_path)
);
INSERT INTO music_refresh_job_entries (job_id, relative_path, entry_kind, state)
SELECT job_id, relative_path, entry_kind, state FROM temp.music_refresh_job_entries_copy;
CREATE INDEX idx_music_refresh_job_entries_pending
    ON music_refresh_job_entries(job_id, entry_kind, state, relative_path);

CREATE TABLE music_refresh_job_issues (
    id TEXT PRIMARY KEY CHECK (trim(id) <> ''),
    job_id TEXT NOT NULL REFERENCES music_refresh_jobs(id) ON DELETE CASCADE,
    issue_code TEXT NOT NULL CHECK (trim(issue_code) <> ''),
    relative_path TEXT,
    item_id TEXT REFERENCES music_library_items(id) ON DELETE SET NULL,
    message TEXT NOT NULL CHECK (trim(message) <> ''),
    created_at_ms INTEGER NOT NULL CHECK (created_at_ms > 0)
);
INSERT INTO music_refresh_job_issues (
    id, job_id, issue_code, relative_path, item_id, message, created_at_ms
)
SELECT id, job_id, issue_code, relative_path, item_id, message, created_at_ms
FROM temp.music_refresh_job_issues_copy;
CREATE INDEX idx_music_refresh_job_issues_job
    ON music_refresh_job_issues(job_id, created_at_ms, id);

DROP TABLE temp.music_refresh_job_issues_copy;
DROP TABLE temp.music_refresh_job_entries_copy;
DROP TABLE temp.music_refresh_jobs_copy;
DROP TABLE temp.music_source_collection_items_copy;

-- Generated noise soundscapes are seeded under fixed keys on every device, so the key names the
-- noise kind and two devices can never hold two rows of one kind.
DROP INDEX idx_music_soundscapes_generated_kind;

CREATE TRIGGER music_soundscapes_generated_identity_insert
BEFORE INSERT ON music_soundscapes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.generated_kind IS NOT NULL
    AND NEW.id IS NOT 'generated-' || NEW.generated_kind || '-noise'
BEGIN
    SELECT RAISE(ABORT, 'Generated noise soundscapes use their fixed key');
END;

CREATE TRIGGER music_soundscapes_generated_identity_update
BEFORE UPDATE OF id, generated_kind ON music_soundscapes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND NEW.generated_kind IS NOT NULL
    AND NEW.id IS NOT 'generated-' || NEW.generated_kind || '-noise'
BEGIN
    SELECT RAISE(ABORT, 'Generated noise soundscapes use their fixed key');
END;

-- Sync triggers rendered by ganbaru-sync from the vault manifest; the conformance test fails
-- when these differ from the current rendering.
CREATE TRIGGER sync_values_music_local_roots_insert
BEFORE INSERT ON music_local_roots
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_local_roots_update
BEFORE UPDATE OF created_at_ms, name, updated_at_ms ON music_local_roots
WHEN ((typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_local_roots
BEFORE UPDATE OF id, created_at_ms ON music_local_roots
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at_ms IS NOT NEW.created_at_ms) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_local_roots
BEFORE INSERT ON music_local_roots
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 48 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_local_roots WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_local_roots_insert
AFTER INSERT ON music_local_roots
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (48, NEW.id, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_local_roots_update
AFTER UPDATE ON music_local_roots
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 48, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_local_roots_delete
AFTER DELETE ON music_local_roots
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (48, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_library_items_insert
BEFORE INSERT ON music_library_items
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.source_kind) = 'text' AND instr(CAST(NEW.source_kind AS BLOB), x'00') = 0 AND length(NEW.source_kind) <= 32 AND length(CAST(NEW.source_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.identity_key) = 'text' AND instr(CAST(NEW.identity_key AS BLOB), x'00') = 0 AND length(trim(NEW.identity_key)) BETWEEN 1 AND 8192 AND length(CAST(NEW.identity_key AS BLOB)) <= 262144)
    AND (NEW.youtube_video_id IS NULL OR (typeof(NEW.youtube_video_id) = 'text' AND instr(CAST(NEW.youtube_video_id AS BLOB), x'00') = 0 AND length(NEW.youtube_video_id) <= 128 AND length(CAST(NEW.youtube_video_id AS BLOB)) <= 262144))
    AND (typeof(NEW.discovered_at_ms) = 'integer' AND NEW.discovered_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.media_kind) = 'text' AND instr(CAST(NEW.media_kind AS BLOB), x'00') = 0 AND length(NEW.media_kind) <= 32 AND length(CAST(NEW.media_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.original_title) = 'text' AND instr(CAST(NEW.original_title AS BLOB), x'00') = 0 AND length(NEW.original_title) <= 500 AND length(CAST(NEW.original_title AS BLOB)) <= 262144)
    AND (typeof(NEW.original_artist) = 'text' AND instr(CAST(NEW.original_artist AS BLOB), x'00') = 0 AND length(NEW.original_artist) <= 500 AND length(CAST(NEW.original_artist AS BLOB)) <= 262144)
    AND (typeof(NEW.original_album) = 'text' AND instr(CAST(NEW.original_album AS BLOB), x'00') = 0 AND length(NEW.original_album) <= 500 AND length(CAST(NEW.original_album AS BLOB)) <= 262144)
    AND (NEW.original_track_number IS NULL OR (typeof(NEW.original_track_number) = 'integer' AND NEW.original_track_number BETWEEN 1 AND 9223372036854775807))
    AND (NEW.duration_ms IS NULL OR (typeof(NEW.duration_ms) = 'integer' AND NEW.duration_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.youtube_resolution_state IS NULL OR (typeof(NEW.youtube_resolution_state) = 'text' AND instr(CAST(NEW.youtube_resolution_state AS BLOB), x'00') = 0 AND length(NEW.youtube_resolution_state) <= 32 AND length(CAST(NEW.youtube_resolution_state AS BLOB)) <= 262144))
    AND (NEW.title_override IS NULL OR (typeof(NEW.title_override) = 'text' AND instr(CAST(NEW.title_override AS BLOB), x'00') = 0 AND length(NEW.title_override) <= 500 AND length(CAST(NEW.title_override AS BLOB)) <= 262144))
    AND (NEW.artist_override IS NULL OR (typeof(NEW.artist_override) = 'text' AND instr(CAST(NEW.artist_override AS BLOB), x'00') = 0 AND length(NEW.artist_override) <= 500 AND length(CAST(NEW.artist_override AS BLOB)) <= 262144))
    AND (NEW.album_override IS NULL OR (typeof(NEW.album_override) = 'text' AND instr(CAST(NEW.album_override AS BLOB), x'00') = 0 AND length(NEW.album_override) <= 500 AND length(CAST(NEW.album_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override IS NULL OR (typeof(NEW.artwork_override) = 'text' AND instr(CAST(NEW.artwork_override AS BLOB), x'00') = 0 AND length(NEW.artwork_override) <= 128 AND length(CAST(NEW.artwork_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override_byte_size IS NULL OR (typeof(NEW.artwork_override_byte_size) = 'integer' AND NEW.artwork_override_byte_size BETWEEN 1 AND 10485760))
    AND (typeof(NEW.review_state) = 'text' AND instr(CAST(NEW.review_state AS BLOB), x'00') = 0 AND length(NEW.review_state) <= 32 AND length(CAST(NEW.review_state AS BLOB)) <= 262144)
    AND (NEW.review_changed_at_ms IS NULL OR (typeof(NEW.review_changed_at_ms) = 'integer' AND NEW.review_changed_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.review_deferred_until_ms IS NULL OR (typeof(NEW.review_deferred_until_ms) = 'integer' AND NEW.review_deferred_until_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_library_items_update
BEFORE UPDATE OF source_kind, identity_key, youtube_video_id, discovered_at_ms, media_kind, original_title, original_artist, original_album, original_track_number, duration_ms, youtube_resolution_state, title_override, artist_override, album_override, artwork_override, artwork_override_byte_size, review_state, review_changed_at_ms, review_deferred_until_ms, updated_at_ms ON music_library_items
WHEN ((typeof(NEW.source_kind) = 'text' AND instr(CAST(NEW.source_kind AS BLOB), x'00') = 0 AND length(NEW.source_kind) <= 32 AND length(CAST(NEW.source_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.identity_key) = 'text' AND instr(CAST(NEW.identity_key AS BLOB), x'00') = 0 AND length(trim(NEW.identity_key)) BETWEEN 1 AND 8192 AND length(CAST(NEW.identity_key AS BLOB)) <= 262144)
    AND (NEW.youtube_video_id IS NULL OR (typeof(NEW.youtube_video_id) = 'text' AND instr(CAST(NEW.youtube_video_id AS BLOB), x'00') = 0 AND length(NEW.youtube_video_id) <= 128 AND length(CAST(NEW.youtube_video_id AS BLOB)) <= 262144))
    AND (typeof(NEW.discovered_at_ms) = 'integer' AND NEW.discovered_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.media_kind) = 'text' AND instr(CAST(NEW.media_kind AS BLOB), x'00') = 0 AND length(NEW.media_kind) <= 32 AND length(CAST(NEW.media_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.original_title) = 'text' AND instr(CAST(NEW.original_title AS BLOB), x'00') = 0 AND length(NEW.original_title) <= 500 AND length(CAST(NEW.original_title AS BLOB)) <= 262144)
    AND (typeof(NEW.original_artist) = 'text' AND instr(CAST(NEW.original_artist AS BLOB), x'00') = 0 AND length(NEW.original_artist) <= 500 AND length(CAST(NEW.original_artist AS BLOB)) <= 262144)
    AND (typeof(NEW.original_album) = 'text' AND instr(CAST(NEW.original_album AS BLOB), x'00') = 0 AND length(NEW.original_album) <= 500 AND length(CAST(NEW.original_album AS BLOB)) <= 262144)
    AND (NEW.original_track_number IS NULL OR (typeof(NEW.original_track_number) = 'integer' AND NEW.original_track_number BETWEEN 1 AND 9223372036854775807))
    AND (NEW.duration_ms IS NULL OR (typeof(NEW.duration_ms) = 'integer' AND NEW.duration_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.youtube_resolution_state IS NULL OR (typeof(NEW.youtube_resolution_state) = 'text' AND instr(CAST(NEW.youtube_resolution_state AS BLOB), x'00') = 0 AND length(NEW.youtube_resolution_state) <= 32 AND length(CAST(NEW.youtube_resolution_state AS BLOB)) <= 262144))
    AND (NEW.title_override IS NULL OR (typeof(NEW.title_override) = 'text' AND instr(CAST(NEW.title_override AS BLOB), x'00') = 0 AND length(NEW.title_override) <= 500 AND length(CAST(NEW.title_override AS BLOB)) <= 262144))
    AND (NEW.artist_override IS NULL OR (typeof(NEW.artist_override) = 'text' AND instr(CAST(NEW.artist_override AS BLOB), x'00') = 0 AND length(NEW.artist_override) <= 500 AND length(CAST(NEW.artist_override AS BLOB)) <= 262144))
    AND (NEW.album_override IS NULL OR (typeof(NEW.album_override) = 'text' AND instr(CAST(NEW.album_override AS BLOB), x'00') = 0 AND length(NEW.album_override) <= 500 AND length(CAST(NEW.album_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override IS NULL OR (typeof(NEW.artwork_override) = 'text' AND instr(CAST(NEW.artwork_override AS BLOB), x'00') = 0 AND length(NEW.artwork_override) <= 128 AND length(CAST(NEW.artwork_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override_byte_size IS NULL OR (typeof(NEW.artwork_override_byte_size) = 'integer' AND NEW.artwork_override_byte_size BETWEEN 1 AND 10485760))
    AND (typeof(NEW.review_state) = 'text' AND instr(CAST(NEW.review_state AS BLOB), x'00') = 0 AND length(NEW.review_state) <= 32 AND length(CAST(NEW.review_state AS BLOB)) <= 262144)
    AND (NEW.review_changed_at_ms IS NULL OR (typeof(NEW.review_changed_at_ms) = 'integer' AND NEW.review_changed_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.review_deferred_until_ms IS NULL OR (typeof(NEW.review_deferred_until_ms) = 'integer' AND NEW.review_deferred_until_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_library_items
BEFORE UPDATE OF id, source_kind, identity_key, youtube_video_id ON music_library_items
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.source_kind IS NOT NEW.source_kind COLLATE BINARY OR OLD.identity_key IS NOT NEW.identity_key COLLATE BINARY OR OLD.youtube_video_id IS NOT NEW.youtube_video_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_library_items
BEFORE INSERT ON music_library_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 49 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_library_items WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_library_items_insert
AFTER INSERT ON music_library_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (49, NEW.id, 511, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_library_items_update
AFTER UPDATE ON music_library_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 49, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.discovered_at_ms IS NOT NEW.discovered_at_ms
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.media_kind IS NOT NEW.media_kind COLLATE BINARY OR OLD.original_title IS NOT NEW.original_title COLLATE BINARY OR OLD.original_artist IS NOT NEW.original_artist COLLATE BINARY OR OLD.original_album IS NOT NEW.original_album COLLATE BINARY OR OLD.original_track_number IS NOT NEW.original_track_number OR OLD.duration_ms IS NOT NEW.duration_ms OR OLD.youtube_resolution_state IS NOT NEW.youtube_resolution_state COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.title_override IS NOT NEW.title_override COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.artist_override IS NOT NEW.artist_override COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.album_override IS NOT NEW.album_override COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.artwork_override IS NOT NEW.artwork_override COLLATE BINARY OR OLD.artwork_override_byte_size IS NOT NEW.artwork_override_byte_size
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.review_state IS NOT NEW.review_state COLLATE BINARY OR OLD.review_changed_at_ms IS NOT NEW.review_changed_at_ms OR OLD.review_deferred_until_ms IS NOT NEW.review_deferred_until_ms
                THEN 128 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 256 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_library_items_delete
AFTER DELETE ON music_library_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (49, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_item_signals_insert
BEFORE INSERT ON music_item_signals
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.signal) = 'text' AND instr(CAST(NEW.signal AS BLOB), x'00') = 0 AND length(NEW.signal) <= 32 AND length(CAST(NEW.signal AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_item_signals_update
BEFORE UPDATE OF item_id, signal, created_at_ms ON music_item_signals
WHEN ((NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.signal) = 'text' AND instr(CAST(NEW.signal AS BLOB), x'00') = 0 AND length(NEW.signal) <= 32 AND length(CAST(NEW.signal AS BLOB)) <= 262144)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_item_signals
BEFORE UPDATE OF sync_key, item_id, signal ON music_item_signals
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.item_id IS NOT NEW.item_id COLLATE BINARY OR OLD.signal IS NOT NEW.signal COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_music_item_signals_insert
AFTER INSERT ON music_item_signals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (50, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_item_signals_update
AFTER UPDATE ON music_item_signals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 50, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at_ms IS NOT NEW.created_at_ms
                THEN 2 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 50, NEW.sync_key, 7, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_item_signals_delete
AFTER DELETE ON music_item_signals
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (50, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_listening_statistics_insert
BEFORE INSERT ON music_listening_statistics
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 128 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (NEW.last_played_at_ms IS NULL OR (typeof(NEW.last_played_at_ms) = 'integer' AND NEW.last_played_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (typeof(NEW.play_count) = 'integer' AND NEW.play_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.completion_count) = 'integer' AND NEW.completion_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.skip_count) = 'integer' AND NEW.skip_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_listening_statistics_update
BEFORE UPDATE OF device_id, item_id, last_played_at_ms, play_count, completion_count, skip_count, updated_at_ms ON music_listening_statistics
WHEN ((typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 128 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (NEW.last_played_at_ms IS NULL OR (typeof(NEW.last_played_at_ms) = 'integer' AND NEW.last_played_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (typeof(NEW.play_count) = 'integer' AND NEW.play_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.completion_count) = 'integer' AND NEW.completion_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.skip_count) = 'integer' AND NEW.skip_count BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_listening_statistics
BEFORE UPDATE OF sync_key, device_id, item_id ON music_listening_statistics
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.device_id IS NOT NEW.device_id COLLATE BINARY OR OLD.item_id IS NOT NEW.item_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_music_listening_statistics_insert
AFTER INSERT ON music_listening_statistics
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (51, NEW.sync_key, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_listening_statistics_update
AFTER UPDATE ON music_listening_statistics
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 51, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.last_played_at_ms IS NOT NEW.last_played_at_ms
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.play_count IS NOT NEW.play_count
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.completion_count IS NOT NEW.completion_count
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.skip_count IS NOT NEW.skip_count
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 51, NEW.sync_key, 127, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_listening_statistics_delete
AFTER DELETE ON music_listening_statistics
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (51, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_local_locations_insert
BEFORE INSERT ON music_local_locations
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.root_id IS NULL OR (typeof(NEW.root_id) = 'text' AND instr(CAST(NEW.root_id AS BLOB), x'00') = 0 AND trim(NEW.root_id) <> '' AND length(CAST(NEW.root_id AS BLOB)) <= 128))
    AND (typeof(NEW.relative_path) = 'text' AND instr(CAST(NEW.relative_path AS BLOB), x'00') = 0 AND length(trim(NEW.relative_path)) BETWEEN 1 AND 4096 AND length(CAST(NEW.relative_path AS BLOB)) <= 262144)
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.first_seen_at_ms) = 'integer' AND NEW.first_seen_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.lightweight_fingerprint IS NULL OR (typeof(NEW.lightweight_fingerprint) = 'text' AND instr(CAST(NEW.lightweight_fingerprint AS BLOB), x'00') = 0 AND length(NEW.lightweight_fingerprint) <= 1024 AND length(CAST(NEW.lightweight_fingerprint AS BLOB)) <= 262144))
    AND (NEW.strong_fingerprint IS NULL OR (typeof(NEW.strong_fingerprint) = 'text' AND instr(CAST(NEW.strong_fingerprint AS BLOB), x'00') = 0 AND length(NEW.strong_fingerprint) <= 1024 AND length(CAST(NEW.strong_fingerprint AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_local_locations_update
BEFORE UPDATE OF root_id, relative_path, item_id, first_seen_at_ms, lightweight_fingerprint, strong_fingerprint, updated_at_ms ON music_local_locations
WHEN ((NEW.root_id IS NULL OR (typeof(NEW.root_id) = 'text' AND instr(CAST(NEW.root_id AS BLOB), x'00') = 0 AND trim(NEW.root_id) <> '' AND length(CAST(NEW.root_id AS BLOB)) <= 128))
    AND (typeof(NEW.relative_path) = 'text' AND instr(CAST(NEW.relative_path AS BLOB), x'00') = 0 AND length(trim(NEW.relative_path)) BETWEEN 1 AND 4096 AND length(CAST(NEW.relative_path AS BLOB)) <= 262144)
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.first_seen_at_ms) = 'integer' AND NEW.first_seen_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.lightweight_fingerprint IS NULL OR (typeof(NEW.lightweight_fingerprint) = 'text' AND instr(CAST(NEW.lightweight_fingerprint AS BLOB), x'00') = 0 AND length(NEW.lightweight_fingerprint) <= 1024 AND length(CAST(NEW.lightweight_fingerprint AS BLOB)) <= 262144))
    AND (NEW.strong_fingerprint IS NULL OR (typeof(NEW.strong_fingerprint) = 'text' AND instr(CAST(NEW.strong_fingerprint AS BLOB), x'00') = 0 AND length(NEW.strong_fingerprint) <= 1024 AND length(CAST(NEW.strong_fingerprint AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_local_locations
BEFORE UPDATE OF id, root_id, relative_path ON music_local_locations
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.root_id IS NOT NEW.root_id COLLATE BINARY OR OLD.relative_path IS NOT NEW.relative_path COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_local_locations
BEFORE INSERT ON music_local_locations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 52 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_local_locations WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_local_locations_insert
AFTER INSERT ON music_local_locations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (52, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_local_locations_update
AFTER UPDATE ON music_local_locations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 52, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.item_id IS NOT NEW.item_id COLLATE BINARY
                AND NOT (NEW.item_id IS NULL AND NOT EXISTS (SELECT 1 FROM music_library_items WHERE id = OLD.item_id))
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.first_seen_at_ms IS NOT NEW.first_seen_at_ms
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.lightweight_fingerprint IS NOT NEW.lightweight_fingerprint COLLATE BINARY OR OLD.strong_fingerprint IS NOT NEW.strong_fingerprint COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_local_locations_delete
AFTER DELETE ON music_local_locations
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (52, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_source_collections_insert
BEFORE INSERT ON music_source_collections
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.kind) = 'text' AND instr(CAST(NEW.kind AS BLOB), x'00') = 0 AND length(NEW.kind) <= 32 AND length(CAST(NEW.kind AS BLOB)) <= 262144)
    AND (typeof(NEW.identity_key) = 'text' AND instr(CAST(NEW.identity_key AS BLOB), x'00') = 0 AND length(trim(NEW.identity_key)) BETWEEN 1 AND 8192 AND length(CAST(NEW.identity_key AS BLOB)) <= 262144)
    AND (NEW.local_root_id IS NULL OR (typeof(NEW.local_root_id) = 'text' AND instr(CAST(NEW.local_root_id AS BLOB), x'00') = 0 AND trim(NEW.local_root_id) <> '' AND length(CAST(NEW.local_root_id AS BLOB)) <= 128))
    AND (NEW.youtube_playlist_id IS NULL OR (typeof(NEW.youtube_playlist_id) = 'text' AND instr(CAST(NEW.youtube_playlist_id AS BLOB), x'00') = 0 AND length(NEW.youtube_playlist_id) <= 128 AND length(CAST(NEW.youtube_playlist_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 500 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.discovery_enabled) = 'integer' AND NEW.discovery_enabled IN (0, 1))
    AND (NEW.removed_at_ms IS NULL OR (typeof(NEW.removed_at_ms) = 'integer' AND NEW.removed_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_source_collections_update
BEFORE UPDATE OF kind, identity_key, local_root_id, youtube_playlist_id, created_at_ms, name, discovery_enabled, removed_at_ms, updated_at_ms ON music_source_collections
WHEN ((typeof(NEW.kind) = 'text' AND instr(CAST(NEW.kind AS BLOB), x'00') = 0 AND length(NEW.kind) <= 32 AND length(CAST(NEW.kind AS BLOB)) <= 262144)
    AND (typeof(NEW.identity_key) = 'text' AND instr(CAST(NEW.identity_key AS BLOB), x'00') = 0 AND length(trim(NEW.identity_key)) BETWEEN 1 AND 8192 AND length(CAST(NEW.identity_key AS BLOB)) <= 262144)
    AND (NEW.local_root_id IS NULL OR (typeof(NEW.local_root_id) = 'text' AND instr(CAST(NEW.local_root_id AS BLOB), x'00') = 0 AND trim(NEW.local_root_id) <> '' AND length(CAST(NEW.local_root_id AS BLOB)) <= 128))
    AND (NEW.youtube_playlist_id IS NULL OR (typeof(NEW.youtube_playlist_id) = 'text' AND instr(CAST(NEW.youtube_playlist_id AS BLOB), x'00') = 0 AND length(NEW.youtube_playlist_id) <= 128 AND length(CAST(NEW.youtube_playlist_id AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 500 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.discovery_enabled) = 'integer' AND NEW.discovery_enabled IN (0, 1))
    AND (NEW.removed_at_ms IS NULL OR (typeof(NEW.removed_at_ms) = 'integer' AND NEW.removed_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_source_collections
BEFORE UPDATE OF id, kind, identity_key, local_root_id, youtube_playlist_id ON music_source_collections
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.kind IS NOT NEW.kind COLLATE BINARY OR OLD.identity_key IS NOT NEW.identity_key COLLATE BINARY OR OLD.local_root_id IS NOT NEW.local_root_id COLLATE BINARY OR OLD.youtube_playlist_id IS NOT NEW.youtube_playlist_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_source_collections
BEFORE INSERT ON music_source_collections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 53 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_source_collections WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_source_collections_insert
AFTER INSERT ON music_source_collections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (53, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_source_collections_update
AFTER UPDATE ON music_source_collections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 53, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at_ms IS NOT NEW.created_at_ms
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.discovery_enabled IS NOT NEW.discovery_enabled
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.removed_at_ms IS NOT NEW.removed_at_ms
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_source_collections_delete
AFTER DELETE ON music_source_collections
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (53, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_source_collection_items_insert
BEFORE INSERT ON music_source_collection_items
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.collection_id IS NULL OR (typeof(NEW.collection_id) = 'text' AND instr(CAST(NEW.collection_id AS BLOB), x'00') = 0 AND trim(NEW.collection_id) <> '' AND length(CAST(NEW.collection_id AS BLOB)) <= 128))
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.first_discovered_at_ms) = 'integer' AND NEW.first_discovered_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.source_position IS NULL OR (typeof(NEW.source_position) = 'integer' AND NEW.source_position BETWEEN 0 AND 9223372036854775807))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_source_collection_items_update
BEFORE UPDATE OF collection_id, item_id, first_discovered_at_ms, source_position ON music_source_collection_items
WHEN ((NEW.collection_id IS NULL OR (typeof(NEW.collection_id) = 'text' AND instr(CAST(NEW.collection_id AS BLOB), x'00') = 0 AND trim(NEW.collection_id) <> '' AND length(CAST(NEW.collection_id AS BLOB)) <= 128))
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.first_discovered_at_ms) = 'integer' AND NEW.first_discovered_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.source_position IS NULL OR (typeof(NEW.source_position) = 'integer' AND NEW.source_position BETWEEN 0 AND 9223372036854775807))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_source_collection_items
BEFORE UPDATE OF sync_key, collection_id, item_id ON music_source_collection_items
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.collection_id IS NOT NEW.collection_id COLLATE BINARY OR OLD.item_id IS NOT NEW.item_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_music_source_collection_items_insert
AFTER INSERT ON music_source_collection_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (54, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_source_collection_items_update
AFTER UPDATE ON music_source_collection_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 54, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.first_discovered_at_ms IS NOT NEW.first_discovered_at_ms
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.source_position IS NOT NEW.source_position
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 54, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_source_collection_items_delete
AFTER DELETE ON music_source_collection_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (54, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_playlists_insert
BEFORE INSERT ON music_playlists
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (typeof(NEW.shuffle_enabled) = 'integer' AND NEW.shuffle_enabled IN (0, 1))
    AND (typeof(NEW.repeat_mode) = 'text' AND instr(CAST(NEW.repeat_mode AS BLOB), x'00') = 0 AND length(NEW.repeat_mode) <= 32 AND length(CAST(NEW.repeat_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.mix_enabled) = 'integer' AND NEW.mix_enabled IN (0, 1))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_playlists_update
BEFORE UPDATE OF created_at_ms, name, icon, shuffle_enabled, repeat_mode, mix_enabled, order_key, updated_at_ms ON music_playlists
WHEN ((typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(NEW.icon) <= 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (typeof(NEW.shuffle_enabled) = 'integer' AND NEW.shuffle_enabled IN (0, 1))
    AND (typeof(NEW.repeat_mode) = 'text' AND instr(CAST(NEW.repeat_mode AS BLOB), x'00') = 0 AND length(NEW.repeat_mode) <= 32 AND length(CAST(NEW.repeat_mode AS BLOB)) <= 262144)
    AND (typeof(NEW.mix_enabled) = 'integer' AND NEW.mix_enabled IN (0, 1))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_playlists
BEFORE UPDATE OF id, created_at_ms ON music_playlists
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at_ms IS NOT NEW.created_at_ms) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_playlists
BEFORE INSERT ON music_playlists
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 55 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_playlists WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_playlists_insert
AFTER INSERT ON music_playlists
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (55, NEW.id, 63, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_playlists_update
AFTER UPDATE ON music_playlists
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 55, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.shuffle_enabled IS NOT NEW.shuffle_enabled OR OLD.repeat_mode IS NOT NEW.repeat_mode COLLATE BINARY OR OLD.mix_enabled IS NOT NEW.mix_enabled
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 32 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_playlists_delete
AFTER DELETE ON music_playlists
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (55, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_playlist_intended_uses_insert
BEFORE INSERT ON music_playlist_intended_uses
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND trim(NEW.playlist_id) <> '' AND length(CAST(NEW.playlist_id AS BLOB)) <= 128))
    AND (typeof(NEW.intended_use) = 'text' AND instr(CAST(NEW.intended_use AS BLOB), x'00') = 0 AND length(NEW.intended_use) <= 32 AND length(CAST(NEW.intended_use AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_playlist_intended_uses_update
BEFORE UPDATE OF playlist_id, intended_use ON music_playlist_intended_uses
WHEN ((NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND trim(NEW.playlist_id) <> '' AND length(CAST(NEW.playlist_id AS BLOB)) <= 128))
    AND (typeof(NEW.intended_use) = 'text' AND instr(CAST(NEW.intended_use AS BLOB), x'00') = 0 AND length(NEW.intended_use) <= 32 AND length(CAST(NEW.intended_use AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_playlist_intended_uses
BEFORE UPDATE OF sync_key, playlist_id, intended_use ON music_playlist_intended_uses
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.playlist_id IS NOT NEW.playlist_id COLLATE BINARY OR OLD.intended_use IS NOT NEW.intended_use COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_music_playlist_intended_uses_insert
AFTER INSERT ON music_playlist_intended_uses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (56, NEW.sync_key, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_playlist_intended_uses_update
AFTER UPDATE ON music_playlist_intended_uses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 56, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            0
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 56, NEW.sync_key, 3, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_playlist_intended_uses_delete
AFTER DELETE ON music_playlist_intended_uses
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (56, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_playlist_memberships_insert
BEFORE INSERT ON music_playlist_memberships
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND trim(NEW.playlist_id) <> '' AND length(CAST(NEW.playlist_id AS BLOB)) <= 128))
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.weight) = 'text' AND instr(CAST(NEW.weight AS BLOB), x'00') = 0 AND length(NEW.weight) <= 32 AND length(CAST(NEW.weight AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (NEW.start_ms IS NULL OR (typeof(NEW.start_ms) = 'integer' AND NEW.start_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.end_ms IS NULL OR (typeof(NEW.end_ms) = 'integer' AND NEW.end_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.volume_milli IS NULL OR (typeof(NEW.volume_milli) = 'integer' AND NEW.volume_milli BETWEEN 0 AND 1000))
    AND (NEW.rate_milli IS NULL OR (typeof(NEW.rate_milli) = 'integer' AND NEW.rate_milli BETWEEN 250 AND 2000))
    AND (NEW.break_item_id IS NULL OR (typeof(NEW.break_item_id) = 'text' AND instr(CAST(NEW.break_item_id AS BLOB), x'00') = 0 AND trim(NEW.break_item_id) <> '' AND length(CAST(NEW.break_item_id AS BLOB)) <= 128))
    AND (NEW.break_start_ms IS NULL OR (typeof(NEW.break_start_ms) = 'integer' AND NEW.break_start_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.break_end_ms IS NULL OR (typeof(NEW.break_end_ms) = 'integer' AND NEW.break_end_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.break_volume_milli IS NULL OR (typeof(NEW.break_volume_milli) = 'integer' AND NEW.break_volume_milli BETWEEN 0 AND 1000))
    AND (NEW.break_rate_milli IS NULL OR (typeof(NEW.break_rate_milli) = 'integer' AND NEW.break_rate_milli BETWEEN 250 AND 2000))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_playlist_memberships_update
BEFORE UPDATE OF playlist_id, item_id, created_at_ms, order_key, weight, enabled, start_ms, end_ms, volume_milli, rate_milli, break_item_id, break_start_ms, break_end_ms, break_volume_milli, break_rate_milli, updated_at_ms ON music_playlist_memberships
WHEN ((NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND trim(NEW.playlist_id) <> '' AND length(CAST(NEW.playlist_id AS BLOB)) <= 128))
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.weight) = 'text' AND instr(CAST(NEW.weight AS BLOB), x'00') = 0 AND length(NEW.weight) <= 32 AND length(CAST(NEW.weight AS BLOB)) <= 262144)
    AND (typeof(NEW.enabled) = 'integer' AND NEW.enabled IN (0, 1))
    AND (NEW.start_ms IS NULL OR (typeof(NEW.start_ms) = 'integer' AND NEW.start_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.end_ms IS NULL OR (typeof(NEW.end_ms) = 'integer' AND NEW.end_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.volume_milli IS NULL OR (typeof(NEW.volume_milli) = 'integer' AND NEW.volume_milli BETWEEN 0 AND 1000))
    AND (NEW.rate_milli IS NULL OR (typeof(NEW.rate_milli) = 'integer' AND NEW.rate_milli BETWEEN 250 AND 2000))
    AND (NEW.break_item_id IS NULL OR (typeof(NEW.break_item_id) = 'text' AND instr(CAST(NEW.break_item_id AS BLOB), x'00') = 0 AND trim(NEW.break_item_id) <> '' AND length(CAST(NEW.break_item_id AS BLOB)) <= 128))
    AND (NEW.break_start_ms IS NULL OR (typeof(NEW.break_start_ms) = 'integer' AND NEW.break_start_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.break_end_ms IS NULL OR (typeof(NEW.break_end_ms) = 'integer' AND NEW.break_end_ms BETWEEN 0 AND 9223372036854775807))
    AND (NEW.break_volume_milli IS NULL OR (typeof(NEW.break_volume_milli) = 'integer' AND NEW.break_volume_milli BETWEEN 0 AND 1000))
    AND (NEW.break_rate_milli IS NULL OR (typeof(NEW.break_rate_milli) = 'integer' AND NEW.break_rate_milli BETWEEN 250 AND 2000))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_playlist_memberships
BEFORE UPDATE OF id, playlist_id, item_id ON music_playlist_memberships
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.playlist_id IS NOT NEW.playlist_id COLLATE BINARY OR OLD.item_id IS NOT NEW.item_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_music_playlist_memberships_insert
AFTER INSERT ON music_playlist_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (57, NEW.id, 2047, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_playlist_memberships_update
AFTER UPDATE ON music_playlist_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 57, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at_ms IS NOT NEW.created_at_ms
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.weight IS NOT NEW.weight COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.enabled IS NOT NEW.enabled
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.start_ms IS NOT NEW.start_ms OR OLD.end_ms IS NOT NEW.end_ms OR OLD.volume_milli IS NOT NEW.volume_milli OR OLD.rate_milli IS NOT NEW.rate_milli
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.break_item_id IS NOT NEW.break_item_id COLLATE BINARY
                AND NOT (NEW.break_item_id IS NULL AND NOT EXISTS (SELECT 1 FROM music_library_items WHERE id = OLD.break_item_id))
                THEN 64 ELSE 0 END)
            | (CASE WHEN OLD.break_start_ms IS NOT NEW.break_start_ms OR OLD.break_end_ms IS NOT NEW.break_end_ms OR OLD.break_volume_milli IS NOT NEW.break_volume_milli OR OLD.break_rate_milli IS NOT NEW.break_rate_milli
                THEN 128 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 512 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_playlist_memberships_delete
AFTER DELETE ON music_playlist_memberships
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (57, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_membership_skip_ranges_insert
BEFORE INSERT ON music_membership_skip_ranges
WHEN ((typeof(NEW.start_ms) = 'integer' AND NEW.start_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_ms) = 'integer' AND NEW.end_ms BETWEEN 0 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_membership_skip_ranges_update
BEFORE UPDATE OF start_ms, end_ms ON music_membership_skip_ranges
WHEN ((typeof(NEW.start_ms) = 'integer' AND NEW.start_ms BETWEEN 0 AND 9223372036854775807)
    AND (typeof(NEW.end_ms) = 'integer' AND NEW.end_ms BETWEEN 0 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_music_membership_skip_ranges_insert
AFTER INSERT ON music_membership_skip_ranges
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (57, NEW.membership_id, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_membership_skip_ranges_update
AFTER UPDATE ON music_membership_skip_ranges
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 57, parent.key, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (SELECT NEW.membership_id AS key UNION SELECT OLD.membership_id) AS parent
    WHERE true
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_membership_skip_ranges_delete
AFTER DELETE ON music_membership_skip_ranges
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (57, OLD.membership_id, 256, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_values_music_snoozes_insert
BEFORE INSERT ON music_snoozes
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.scope) = 'text' AND instr(CAST(NEW.scope AS BLOB), x'00') = 0 AND length(NEW.scope) <= 32 AND length(CAST(NEW.scope AS BLOB)) <= 262144)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND trim(NEW.playlist_id) <> '' AND length(CAST(NEW.playlist_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.starts_at_ms) = 'integer' AND NEW.starts_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.ends_at_ms IS NULL OR (typeof(NEW.ends_at_ms) = 'integer' AND NEW.ends_at_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 500 AND length(CAST(NEW.reason AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_snoozes_update
BEFORE UPDATE OF item_id, scope, playlist_id, created_at_ms, starts_at_ms, ends_at_ms, reason ON music_snoozes
WHEN ((NEW.item_id IS NULL OR (typeof(NEW.item_id) = 'text' AND instr(CAST(NEW.item_id AS BLOB), x'00') = 0 AND trim(NEW.item_id) <> '' AND length(CAST(NEW.item_id AS BLOB)) <= 128))
    AND (typeof(NEW.scope) = 'text' AND instr(CAST(NEW.scope AS BLOB), x'00') = 0 AND length(NEW.scope) <= 32 AND length(CAST(NEW.scope AS BLOB)) <= 262144)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND trim(NEW.playlist_id) <> '' AND length(CAST(NEW.playlist_id AS BLOB)) <= 128))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.starts_at_ms) = 'integer' AND NEW.starts_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.ends_at_ms IS NULL OR (typeof(NEW.ends_at_ms) = 'integer' AND NEW.ends_at_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.reason) = 'text' AND instr(CAST(NEW.reason AS BLOB), x'00') = 0 AND length(NEW.reason) <= 500 AND length(CAST(NEW.reason AS BLOB)) <= 262144)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_snoozes
BEFORE UPDATE OF id, item_id, scope, playlist_id ON music_snoozes
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.item_id IS NOT NEW.item_id COLLATE BINARY OR OLD.scope IS NOT NEW.scope COLLATE BINARY OR OLD.playlist_id IS NOT NEW.playlist_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_snoozes
BEFORE INSERT ON music_snoozes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 58 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_snoozes WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_snoozes_insert
AFTER INSERT ON music_snoozes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (58, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_snoozes_update
AFTER UPDATE ON music_snoozes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 58, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at_ms IS NOT NEW.created_at_ms
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.starts_at_ms IS NOT NEW.starts_at_ms OR OLD.ends_at_ms IS NOT NEW.ends_at_ms
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.reason IS NOT NEW.reason COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_snoozes_delete
AFTER DELETE ON music_snoozes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (58, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_soundscapes_insert
BEFORE INSERT ON music_soundscapes
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.source_kind) = 'text' AND instr(CAST(NEW.source_kind AS BLOB), x'00') = 0 AND length(NEW.source_kind) <= 32 AND length(CAST(NEW.source_kind AS BLOB)) <= 262144)
    AND (NEW.generated_kind IS NULL OR (typeof(NEW.generated_kind) = 'text' AND instr(CAST(NEW.generated_kind AS BLOB), x'00') = 0 AND length(NEW.generated_kind) <= 32 AND length(CAST(NEW.generated_kind AS BLOB)) <= 262144))
    AND (NEW.bundled_identity IS NULL OR (typeof(NEW.bundled_identity) = 'text' AND instr(CAST(NEW.bundled_identity AS BLOB), x'00') = 0 AND length(NEW.bundled_identity) <= 8192 AND length(CAST(NEW.bundled_identity AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_soundscapes_update
BEFORE UPDATE OF source_kind, generated_kind, bundled_identity, created_at_ms, name, icon, updated_at_ms ON music_soundscapes
WHEN ((typeof(NEW.source_kind) = 'text' AND instr(CAST(NEW.source_kind AS BLOB), x'00') = 0 AND length(NEW.source_kind) <= 32 AND length(CAST(NEW.source_kind AS BLOB)) <= 262144)
    AND (NEW.generated_kind IS NULL OR (typeof(NEW.generated_kind) = 'text' AND instr(CAST(NEW.generated_kind AS BLOB), x'00') = 0 AND length(NEW.generated_kind) <= 32 AND length(CAST(NEW.generated_kind AS BLOB)) <= 262144))
    AND (NEW.bundled_identity IS NULL OR (typeof(NEW.bundled_identity) = 'text' AND instr(CAST(NEW.bundled_identity AS BLOB), x'00') = 0 AND length(NEW.bundled_identity) <= 8192 AND length(CAST(NEW.bundled_identity AS BLOB)) <= 262144))
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 200 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_soundscapes
BEFORE UPDATE OF id, source_kind, generated_kind, bundled_identity ON music_soundscapes
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.source_kind IS NOT NEW.source_kind COLLATE BINARY OR OLD.generated_kind IS NOT NEW.generated_kind COLLATE BINARY OR OLD.bundled_identity IS NOT NEW.bundled_identity COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_soundscapes
BEFORE INSERT ON music_soundscapes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 59 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_soundscapes WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_soundscapes_insert
AFTER INSERT ON music_soundscapes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (59, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_soundscapes_update
AFTER UPDATE ON music_soundscapes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 59, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.created_at_ms IS NOT NEW.created_at_ms
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_soundscapes_delete
AFTER DELETE ON music_soundscapes
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (59, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_soundscape_presets_insert
BEFORE INSERT ON music_soundscape_presets
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 80 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (typeof(NEW.level_milli) = 'integer' AND NEW.level_milli BETWEEN 0 AND 2000)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_soundscape_presets_update
BEFORE UPDATE OF created_at_ms, name, icon, level_milli, updated_at_ms ON music_soundscape_presets
WHEN ((typeof(NEW.created_at_ms) = 'integer' AND NEW.created_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (typeof(NEW.name) = 'text' AND instr(CAST(NEW.name AS BLOB), x'00') = 0 AND length(trim(NEW.name)) BETWEEN 1 AND 80 AND length(CAST(NEW.name AS BLOB)) <= 262144)
    AND (typeof(NEW.icon) = 'text' AND instr(CAST(NEW.icon AS BLOB), x'00') = 0 AND length(trim(NEW.icon)) BETWEEN 1 AND 500 AND length(CAST(NEW.icon AS BLOB)) <= 262144)
    AND (typeof(NEW.level_milli) = 'integer' AND NEW.level_milli BETWEEN 0 AND 2000)
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_soundscape_presets
BEFORE UPDATE OF id, created_at_ms ON music_soundscape_presets
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.created_at_ms IS NOT NEW.created_at_ms) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_music_soundscape_presets
BEFORE INSERT ON music_soundscape_presets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 60 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM music_soundscape_presets WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_music_soundscape_presets_insert
AFTER INSERT ON music_soundscape_presets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (60, NEW.id, 31, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_soundscape_presets_update
AFTER UPDATE ON music_soundscape_presets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 60, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.name IS NOT NEW.name COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.icon IS NOT NEW.icon COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.level_milli IS NOT NEW.level_milli
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 16 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_music_soundscape_presets_delete
AFTER DELETE ON music_soundscape_presets
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (60, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_soundscape_preset_sounds_insert
BEFORE INSERT ON music_soundscape_preset_sounds
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (NEW.preset_id IS NULL OR (typeof(NEW.preset_id) = 'text' AND instr(CAST(NEW.preset_id AS BLOB), x'00') = 0 AND trim(NEW.preset_id) <> '' AND length(CAST(NEW.preset_id AS BLOB)) <= 128))
    AND (NEW.soundscape_id IS NULL OR (typeof(NEW.soundscape_id) = 'text' AND instr(CAST(NEW.soundscape_id AS BLOB), x'00') = 0 AND trim(NEW.soundscape_id) <> '' AND length(CAST(NEW.soundscape_id AS BLOB)) <= 128))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.level_milli) = 'integer' AND NEW.level_milli BETWEEN 0 AND 1000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_soundscape_preset_sounds_update
BEFORE UPDATE OF preset_id, soundscape_id, order_key, level_milli ON music_soundscape_preset_sounds
WHEN ((NEW.preset_id IS NULL OR (typeof(NEW.preset_id) = 'text' AND instr(CAST(NEW.preset_id AS BLOB), x'00') = 0 AND trim(NEW.preset_id) <> '' AND length(CAST(NEW.preset_id AS BLOB)) <= 128))
    AND (NEW.soundscape_id IS NULL OR (typeof(NEW.soundscape_id) = 'text' AND instr(CAST(NEW.soundscape_id AS BLOB), x'00') = 0 AND trim(NEW.soundscape_id) <> '' AND length(CAST(NEW.soundscape_id AS BLOB)) <= 128))
    AND (typeof(NEW.order_key) = 'text' AND instr(CAST(NEW.order_key AS BLOB), x'00') = 0 AND NEW.order_key NOT GLOB '*[^0-9A-Za-z]*' AND length(NEW.order_key) <= 128 AND (CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) <= length(NEW.order_key) AND ((CASE WHEN unicode(NEW.order_key) BETWEEN 97 AND 122 THEN unicode(NEW.order_key) - 95 WHEN unicode(NEW.order_key) BETWEEN 65 AND 90 THEN 92 - unicode(NEW.order_key) ELSE 129 END) = length(NEW.order_key) OR substr(NEW.order_key, -1) <> '0') AND NEW.order_key <> 'A00000000000000000000000000')
    AND (typeof(NEW.level_milli) = 'integer' AND NEW.level_milli BETWEEN 0 AND 1000)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_soundscape_preset_sounds
BEFORE UPDATE OF sync_key, preset_id, soundscape_id ON music_soundscape_preset_sounds
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.preset_id IS NOT NEW.preset_id COLLATE BINARY OR OLD.soundscape_id IS NOT NEW.soundscape_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_music_soundscape_preset_sounds_insert
AFTER INSERT ON music_soundscape_preset_sounds
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (61, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_soundscape_preset_sounds_update
AFTER UPDATE ON music_soundscape_preset_sounds
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 61, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.order_key IS NOT NEW.order_key COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.level_milli IS NOT NEW.level_milli
                THEN 4 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 61, NEW.sync_key, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_soundscape_preset_sounds_delete
AFTER DELETE ON music_soundscape_preset_sounds
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (61, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

CREATE TRIGGER sync_values_music_context_assignments_insert
BEFORE INSERT ON music_context_assignments
WHEN ((typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128)
    AND (typeof(NEW.owner_kind) = 'text' AND instr(CAST(NEW.owner_kind AS BLOB), x'00') = 0 AND length(NEW.owner_kind) <= 32 AND length(CAST(NEW.owner_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.owner_id) = 'text' AND instr(CAST(NEW.owner_id AS BLOB), x'00') = 0 AND length(trim(NEW.owner_id)) BETWEEN 1 AND 128 AND length(CAST(NEW.owner_id AS BLOB)) <= 262144)
    AND (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144)
    AND (typeof(NEW.behavior) = 'text' AND instr(CAST(NEW.behavior AS BLOB), x'00') = 0 AND length(NEW.behavior) <= 32 AND length(CAST(NEW.behavior AS BLOB)) <= 262144)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND length(NEW.playlist_id) <= 128 AND length(CAST(NEW.playlist_id AS BLOB)) <= 262144))
    AND (typeof(NEW.soundscape_behavior) = 'text' AND instr(CAST(NEW.soundscape_behavior AS BLOB), x'00') = 0 AND length(NEW.soundscape_behavior) <= 32 AND length(CAST(NEW.soundscape_behavior AS BLOB)) <= 262144)
    AND (NEW.soundscape_preset_id IS NULL OR (typeof(NEW.soundscape_preset_id) = 'text' AND instr(CAST(NEW.soundscape_preset_id AS BLOB), x'00') = 0 AND length(NEW.soundscape_preset_id) <= 128 AND length(CAST(NEW.soundscape_preset_id AS BLOB)) <= 262144))
    AND (typeof(NEW.provenance_kind) = 'text' AND instr(CAST(NEW.provenance_kind AS BLOB), x'00') = 0 AND length(NEW.provenance_kind) <= 32 AND length(CAST(NEW.provenance_kind AS BLOB)) <= 262144)
    AND (NEW.provenance_id IS NULL OR (typeof(NEW.provenance_id) = 'text' AND instr(CAST(NEW.provenance_id AS BLOB), x'00') = 0 AND length(NEW.provenance_id) <= 128 AND length(CAST(NEW.provenance_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_context_assignments_update
BEFORE UPDATE OF owner_kind, owner_id, phase, behavior, playlist_id, soundscape_behavior, soundscape_preset_id, provenance_kind, provenance_id, updated_at_ms ON music_context_assignments
WHEN ((typeof(NEW.owner_kind) = 'text' AND instr(CAST(NEW.owner_kind AS BLOB), x'00') = 0 AND length(NEW.owner_kind) <= 32 AND length(CAST(NEW.owner_kind AS BLOB)) <= 262144)
    AND (typeof(NEW.owner_id) = 'text' AND instr(CAST(NEW.owner_id AS BLOB), x'00') = 0 AND length(trim(NEW.owner_id)) BETWEEN 1 AND 128 AND length(CAST(NEW.owner_id AS BLOB)) <= 262144)
    AND (typeof(NEW.phase) = 'text' AND instr(CAST(NEW.phase AS BLOB), x'00') = 0 AND length(NEW.phase) <= 32 AND length(CAST(NEW.phase AS BLOB)) <= 262144)
    AND (typeof(NEW.behavior) = 'text' AND instr(CAST(NEW.behavior AS BLOB), x'00') = 0 AND length(NEW.behavior) <= 32 AND length(CAST(NEW.behavior AS BLOB)) <= 262144)
    AND (NEW.playlist_id IS NULL OR (typeof(NEW.playlist_id) = 'text' AND instr(CAST(NEW.playlist_id AS BLOB), x'00') = 0 AND length(NEW.playlist_id) <= 128 AND length(CAST(NEW.playlist_id AS BLOB)) <= 262144))
    AND (typeof(NEW.soundscape_behavior) = 'text' AND instr(CAST(NEW.soundscape_behavior AS BLOB), x'00') = 0 AND length(NEW.soundscape_behavior) <= 32 AND length(CAST(NEW.soundscape_behavior AS BLOB)) <= 262144)
    AND (NEW.soundscape_preset_id IS NULL OR (typeof(NEW.soundscape_preset_id) = 'text' AND instr(CAST(NEW.soundscape_preset_id AS BLOB), x'00') = 0 AND length(NEW.soundscape_preset_id) <= 128 AND length(CAST(NEW.soundscape_preset_id AS BLOB)) <= 262144))
    AND (typeof(NEW.provenance_kind) = 'text' AND instr(CAST(NEW.provenance_kind AS BLOB), x'00') = 0 AND length(NEW.provenance_kind) <= 32 AND length(CAST(NEW.provenance_kind AS BLOB)) <= 262144)
    AND (NEW.provenance_id IS NULL OR (typeof(NEW.provenance_id) = 'text' AND instr(CAST(NEW.provenance_id AS BLOB), x'00') = 0 AND length(NEW.provenance_id) <= 128 AND length(CAST(NEW.provenance_id AS BLOB)) <= 262144))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_music_context_assignments
BEFORE UPDATE OF sync_key, owner_kind, owner_id, phase ON music_context_assignments
WHEN (OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
        AND (OLD.sync_key NOT GLOB 'pending:*' OR (typeof(NEW.sync_key) = 'text' AND instr(CAST(NEW.sync_key AS BLOB), x'00') = 0 AND trim(NEW.sync_key) <> '' AND length(CAST(NEW.sync_key AS BLOB)) <= 128) IS NOT 1 OR NEW.sync_key GLOB 'pending:*'))
    OR ((OLD.owner_kind IS NOT NEW.owner_kind COLLATE BINARY OR OLD.owner_id IS NOT NEW.owner_id COLLATE BINARY OR OLD.phase IS NOT NEW.phase COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_capture_music_context_assignments_insert
AFTER INSERT ON music_context_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (62, NEW.sync_key, 255, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_context_assignments_update
AFTER UPDATE ON music_context_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 62, NEW.sync_key, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.behavior IS NOT NEW.behavior COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.playlist_id IS NOT NEW.playlist_id COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.soundscape_behavior IS NOT NEW.soundscape_behavior COLLATE BINARY
                THEN 8 ELSE 0 END)
            | (CASE WHEN OLD.soundscape_preset_id IS NOT NEW.soundscape_preset_id COLLATE BINARY
                THEN 16 ELSE 0 END)
            | (CASE WHEN OLD.provenance_kind IS NOT NEW.provenance_kind COLLATE BINARY OR OLD.provenance_id IS NOT NEW.provenance_id COLLATE BINARY
                THEN 32 ELSE 0 END)
            | (CASE WHEN OLD.updated_at_ms IS NOT NEW.updated_at_ms
                THEN 64 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 62, NEW.sync_key, 255, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    WHERE OLD.sync_key IS NOT NEW.sync_key COLLATE BINARY
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_music_context_assignments_delete
AFTER DELETE ON music_context_assignments
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (62, OLD.sync_key, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

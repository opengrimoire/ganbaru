-- Music item source metadata and YouTube resolution state merge as separate coupled groups, so a
-- placeholder written for an unresolved video never beats metadata or a resolution another device
-- wrote concurrently. `metadata_resolved` is 0 only for a YouTube placeholder whose title and
-- channel are not known yet. The capture triggers of items are rendered from the sync manifest.

DROP TRIGGER IF EXISTS sync_values_music_library_items_insert;
DROP TRIGGER IF EXISTS sync_values_music_library_items_update;
DROP TRIGGER IF EXISTS sync_capture_music_library_items_insert;
DROP TRIGGER IF EXISTS sync_capture_music_library_items_update;

ALTER TABLE music_library_items
    ADD COLUMN metadata_resolved INTEGER NOT NULL DEFAULT 1 CHECK (metadata_resolved IN (0, 1));

UPDATE music_library_items SET metadata_resolved = 0
WHERE youtube_resolution_state = 'resolving';

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
    AND (typeof(NEW.metadata_resolved) = 'integer' AND NEW.metadata_resolved IN (0, 1))
    AND (NEW.title_override IS NULL OR (typeof(NEW.title_override) = 'text' AND instr(CAST(NEW.title_override AS BLOB), x'00') = 0 AND length(NEW.title_override) <= 500 AND length(CAST(NEW.title_override AS BLOB)) <= 262144))
    AND (NEW.artist_override IS NULL OR (typeof(NEW.artist_override) = 'text' AND instr(CAST(NEW.artist_override AS BLOB), x'00') = 0 AND length(NEW.artist_override) <= 500 AND length(CAST(NEW.artist_override AS BLOB)) <= 262144))
    AND (NEW.album_override IS NULL OR (typeof(NEW.album_override) = 'text' AND instr(CAST(NEW.album_override AS BLOB), x'00') = 0 AND length(NEW.album_override) <= 500 AND length(CAST(NEW.album_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override IS NULL OR (typeof(NEW.artwork_override) = 'text' AND instr(CAST(NEW.artwork_override AS BLOB), x'00') = 0 AND length(NEW.artwork_override) <= 128 AND length(CAST(NEW.artwork_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override_byte_size IS NULL OR (typeof(NEW.artwork_override_byte_size) = 'integer' AND NEW.artwork_override_byte_size BETWEEN 1 AND 10485760))
    AND (typeof(NEW.review_state) = 'text' AND instr(CAST(NEW.review_state AS BLOB), x'00') = 0 AND length(NEW.review_state) <= 32 AND length(CAST(NEW.review_state AS BLOB)) <= 262144)
    AND (NEW.review_changed_at_ms IS NULL OR (typeof(NEW.review_changed_at_ms) = 'integer' AND NEW.review_changed_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.review_deferred_until_ms IS NULL OR (typeof(NEW.review_deferred_until_ms) = 'integer' AND NEW.review_deferred_until_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.youtube_resolution_state IS NULL OR (typeof(NEW.youtube_resolution_state) = 'text' AND instr(CAST(NEW.youtube_resolution_state AS BLOB), x'00') = 0 AND length(NEW.youtube_resolution_state) <= 32 AND length(CAST(NEW.youtube_resolution_state AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_music_library_items_update
BEFORE UPDATE OF source_kind, identity_key, youtube_video_id, discovered_at_ms, media_kind, original_title, original_artist, original_album, original_track_number, duration_ms, metadata_resolved, title_override, artist_override, album_override, artwork_override, artwork_override_byte_size, review_state, review_changed_at_ms, review_deferred_until_ms, updated_at_ms, youtube_resolution_state ON music_library_items
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
    AND (typeof(NEW.metadata_resolved) = 'integer' AND NEW.metadata_resolved IN (0, 1))
    AND (NEW.title_override IS NULL OR (typeof(NEW.title_override) = 'text' AND instr(CAST(NEW.title_override AS BLOB), x'00') = 0 AND length(NEW.title_override) <= 500 AND length(CAST(NEW.title_override AS BLOB)) <= 262144))
    AND (NEW.artist_override IS NULL OR (typeof(NEW.artist_override) = 'text' AND instr(CAST(NEW.artist_override AS BLOB), x'00') = 0 AND length(NEW.artist_override) <= 500 AND length(CAST(NEW.artist_override AS BLOB)) <= 262144))
    AND (NEW.album_override IS NULL OR (typeof(NEW.album_override) = 'text' AND instr(CAST(NEW.album_override AS BLOB), x'00') = 0 AND length(NEW.album_override) <= 500 AND length(CAST(NEW.album_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override IS NULL OR (typeof(NEW.artwork_override) = 'text' AND instr(CAST(NEW.artwork_override AS BLOB), x'00') = 0 AND length(NEW.artwork_override) <= 128 AND length(CAST(NEW.artwork_override AS BLOB)) <= 262144))
    AND (NEW.artwork_override_byte_size IS NULL OR (typeof(NEW.artwork_override_byte_size) = 'integer' AND NEW.artwork_override_byte_size BETWEEN 1 AND 10485760))
    AND (typeof(NEW.review_state) = 'text' AND instr(CAST(NEW.review_state AS BLOB), x'00') = 0 AND length(NEW.review_state) <= 32 AND length(CAST(NEW.review_state AS BLOB)) <= 262144)
    AND (NEW.review_changed_at_ms IS NULL OR (typeof(NEW.review_changed_at_ms) = 'integer' AND NEW.review_changed_at_ms BETWEEN -9223372036854775808 AND 9223372036854775807))
    AND (NEW.review_deferred_until_ms IS NULL OR (typeof(NEW.review_deferred_until_ms) = 'integer' AND NEW.review_deferred_until_ms BETWEEN 1 AND 9223372036854775807))
    AND (typeof(NEW.updated_at_ms) = 'integer' AND NEW.updated_at_ms BETWEEN 1 AND 9223372036854775807)
    AND (NEW.youtube_resolution_state IS NULL OR (typeof(NEW.youtube_resolution_state) = 'text' AND instr(CAST(NEW.youtube_resolution_state AS BLOB), x'00') = 0 AND length(NEW.youtube_resolution_state) <= 32 AND length(CAST(NEW.youtube_resolution_state AS BLOB)) <= 262144))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_capture_music_library_items_insert
AFTER INSERT ON music_library_items
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (49, NEW.id, 1023, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
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
            | (CASE WHEN OLD.media_kind IS NOT NEW.media_kind COLLATE BINARY OR OLD.original_title IS NOT NEW.original_title COLLATE BINARY OR OLD.original_artist IS NOT NEW.original_artist COLLATE BINARY OR OLD.original_album IS NOT NEW.original_album COLLATE BINARY OR OLD.original_track_number IS NOT NEW.original_track_number OR OLD.duration_ms IS NOT NEW.duration_ms OR OLD.metadata_resolved IS NOT NEW.metadata_resolved
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
            | (CASE WHEN OLD.youtube_resolution_state IS NOT NEW.youtube_resolution_state COLLATE BINARY
                THEN 512 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

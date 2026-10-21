-- Original titles, artists, and albums come from media tags, YouTube, and imported files. They
-- are capped at 500 characters, matching the extraction and import truncation.
UPDATE music_library_items
SET original_title = substr(original_title, 1, 500)
WHERE length(original_title) > 500;

UPDATE music_library_items
SET original_artist = substr(original_artist, 1, 500)
WHERE length(original_artist) > 500;

UPDATE music_library_items
SET original_album = substr(original_album, 1, 500)
WHERE length(original_album) > 500;

CREATE TRIGGER IF NOT EXISTS music_library_items_description_limits_insert
BEFORE INSERT ON music_library_items
WHEN length(NEW.original_title) > 500
    OR length(NEW.original_artist) > 500
    OR length(NEW.original_album) > 500
BEGIN
    SELECT RAISE(ABORT, 'Music item description exceeds 500 characters');
END;

CREATE TRIGGER IF NOT EXISTS music_library_items_description_limits_update
BEFORE UPDATE OF original_title, original_artist, original_album ON music_library_items
WHEN length(NEW.original_title) > 500
    OR length(NEW.original_artist) > 500
    OR length(NEW.original_album) > 500
BEGIN
    SELECT RAISE(ABORT, 'Music item description exceeds 500 characters');
END;

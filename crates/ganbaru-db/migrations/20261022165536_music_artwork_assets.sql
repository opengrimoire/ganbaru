-- Music artwork overrides are managed vault assets of the music/artwork class: the item stores
-- the reference 'music/artwork/{sha256}.{ext}' and the exact size of the file, at most 10 MiB.
-- Existing overrides are device paths that cannot name a managed asset, so they are cleared and
-- the item shows its original artwork.

UPDATE music_library_items SET artwork_override = NULL WHERE artwork_override IS NOT NULL;

ALTER TABLE music_library_items ADD COLUMN artwork_override_byte_size INTEGER
    CHECK (
        (artwork_override IS NULL AND artwork_override_byte_size IS NULL)
        OR (
            artwork_override IS NOT NULL
            AND substr(artwork_override, 1, 14) = 'music/artwork/'
            AND artwork_override_byte_size IS NOT NULL
            AND artwork_override_byte_size BETWEEN 1 AND 10485760
        )
    );

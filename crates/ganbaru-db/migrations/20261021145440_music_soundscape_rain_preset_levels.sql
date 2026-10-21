-- Each seeded rain preset starts at its own level relative to the Music volume: Windy rain is the brightest
-- texture and sits lowest, Soft rain is the deepest and sits above Music. Presets the listener already edited
-- keep their level.
UPDATE music_soundscape_presets
SET level = CASE id
        WHEN 'preset-generated-white-noise' THEN 0.5
        WHEN 'preset-generated-pink-noise' THEN 0.75
        WHEN 'preset-generated-brown-noise' THEN 1.25
    END
WHERE id IN ('preset-generated-white-noise', 'preset-generated-pink-noise', 'preset-generated-brown-noise')
    AND version = 1
    AND level = 0.5;

-- Notes page history snapshots name the managed asset ids their page and blocks referenced when
-- they were taken. Snapshot blocks live in history bundles that may be compressed, so the asset
-- sweep reads this column to keep the files a later restore brings back. Snapshots taken before
-- the column existed name nothing.

ALTER TABLE notes_page_history_snapshots
    ADD COLUMN asset_ids TEXT NOT NULL DEFAULT '[]' CHECK (json_valid(asset_ids));

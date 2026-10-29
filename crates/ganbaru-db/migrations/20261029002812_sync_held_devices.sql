-- Whether the selected authorization chain holds an enrolled device for data: a stored branch
-- of the head's frozen epoch instance removes it, so its operations are stored but neither
-- applied nor served past authorization records until a handover resolves the instance.
-- Derived on every selection like the rest of the table.

ALTER TABLE sync_devices ADD COLUMN held INTEGER NOT NULL DEFAULT 0 CHECK (held IN (0, 1));

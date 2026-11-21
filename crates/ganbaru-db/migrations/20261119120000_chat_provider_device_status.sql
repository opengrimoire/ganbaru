-- Each desktop shares, per provider family, whether it can run that family, the model names and
-- option labels of its catalog, and the plan limits it last saw, so a linked device that runs no
-- agents shows what the desktop would run. Only the device a row names writes it, and payloads
-- hold the canonical, bounded JSON of the sync contracts: no account details, raw provider
-- payloads, paths, or executables.
CREATE TABLE chat_provider_device_status (
  id TEXT PRIMARY KEY NOT NULL CHECK (length(id) BETWEEN 1 AND 128),
  device_id TEXT NOT NULL CHECK (length(trim(device_id)) BETWEEN 1 AND 160),
  provider_family_id TEXT NOT NULL CHECK (length(trim(provider_family_id)) BETWEEN 1 AND 1024),
  available INTEGER NOT NULL CHECK (available IN (0, 1)),
  status_changed_at TEXT NOT NULL CHECK (length(status_changed_at) BETWEEN 20 AND 64),
  catalog_data TEXT NOT NULL CHECK (
      length(catalog_data) <= 32768
      AND json_valid(catalog_data)
      AND json_type(catalog_data) = 'object'
  ),
  catalog_changed_at TEXT NOT NULL CHECK (length(catalog_changed_at) BETWEEN 20 AND 64),
  limits_data TEXT CHECK (
      limits_data IS NULL
      OR (
          length(limits_data) <= 16384
          AND json_valid(limits_data)
          AND json_type(limits_data) = 'object'
      )
  ),
  limits_observed_at TEXT CHECK (
      limits_observed_at IS NULL OR length(limits_observed_at) BETWEEN 20 AND 64
  )
) STRICT;

-- Sync triggers rendered from the vault manifest.
CREATE TRIGGER sync_values_chat_provider_device_status_insert
BEFORE INSERT ON chat_provider_device_status
WHEN ((typeof(NEW.id) = 'text' AND instr(CAST(NEW.id AS BLOB), x'00') = 0 AND trim(NEW.id) <> '' AND length(CAST(NEW.id AS BLOB)) <= 128)
    AND (typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (typeof(NEW.provider_family_id) = 'text' AND instr(CAST(NEW.provider_family_id AS BLOB), x'00') = 0 AND length(trim(NEW.provider_family_id)) BETWEEN 1 AND 1024 AND length(CAST(NEW.provider_family_id AS BLOB)) <= 262144)
    AND (typeof(NEW.available) = 'integer' AND NEW.available IN (0, 1))
    AND (typeof(NEW.status_changed_at) = 'text' AND instr(CAST(NEW.status_changed_at AS BLOB), x'00') = 0 AND trim(NEW.status_changed_at) <> '' AND length(CAST(NEW.status_changed_at AS BLOB)) <= 64)
    AND (typeof(NEW.catalog_data) = 'text' AND instr(CAST(NEW.catalog_data AS BLOB), x'00') = 0 AND length(NEW.catalog_data) <= 32768 AND length(CAST(NEW.catalog_data AS BLOB)) <= 262144)
    AND (typeof(NEW.catalog_changed_at) = 'text' AND instr(CAST(NEW.catalog_changed_at AS BLOB), x'00') = 0 AND trim(NEW.catalog_changed_at) <> '' AND length(CAST(NEW.catalog_changed_at AS BLOB)) <= 64)
    AND (NEW.limits_data IS NULL OR (typeof(NEW.limits_data) = 'text' AND instr(CAST(NEW.limits_data AS BLOB), x'00') = 0 AND length(NEW.limits_data) <= 16384 AND length(CAST(NEW.limits_data AS BLOB)) <= 262144))
    AND (NEW.limits_observed_at IS NULL OR (typeof(NEW.limits_observed_at) = 'text' AND instr(CAST(NEW.limits_observed_at AS BLOB), x'00') = 0 AND trim(NEW.limits_observed_at) <> '' AND length(CAST(NEW.limits_observed_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_values_chat_provider_device_status_update
BEFORE UPDATE OF device_id, provider_family_id, available, status_changed_at, catalog_data, catalog_changed_at, limits_data, limits_observed_at ON chat_provider_device_status
WHEN ((typeof(NEW.device_id) = 'text' AND instr(CAST(NEW.device_id AS BLOB), x'00') = 0 AND length(trim(NEW.device_id)) BETWEEN 1 AND 160 AND length(CAST(NEW.device_id AS BLOB)) <= 262144)
    AND (typeof(NEW.provider_family_id) = 'text' AND instr(CAST(NEW.provider_family_id AS BLOB), x'00') = 0 AND length(trim(NEW.provider_family_id)) BETWEEN 1 AND 1024 AND length(CAST(NEW.provider_family_id AS BLOB)) <= 262144)
    AND (typeof(NEW.available) = 'integer' AND NEW.available IN (0, 1))
    AND (typeof(NEW.status_changed_at) = 'text' AND instr(CAST(NEW.status_changed_at AS BLOB), x'00') = 0 AND trim(NEW.status_changed_at) <> '' AND length(CAST(NEW.status_changed_at AS BLOB)) <= 64)
    AND (typeof(NEW.catalog_data) = 'text' AND instr(CAST(NEW.catalog_data AS BLOB), x'00') = 0 AND length(NEW.catalog_data) <= 32768 AND length(CAST(NEW.catalog_data AS BLOB)) <= 262144)
    AND (typeof(NEW.catalog_changed_at) = 'text' AND instr(CAST(NEW.catalog_changed_at AS BLOB), x'00') = 0 AND trim(NEW.catalog_changed_at) <> '' AND length(CAST(NEW.catalog_changed_at AS BLOB)) <= 64)
    AND (NEW.limits_data IS NULL OR (typeof(NEW.limits_data) = 'text' AND instr(CAST(NEW.limits_data AS BLOB), x'00') = 0 AND length(NEW.limits_data) <= 16384 AND length(CAST(NEW.limits_data AS BLOB)) <= 262144))
    AND (NEW.limits_observed_at IS NULL OR (typeof(NEW.limits_observed_at) = 'text' AND instr(CAST(NEW.limits_observed_at AS BLOB), x'00') = 0 AND trim(NEW.limits_observed_at) <> '' AND length(CAST(NEW.limits_observed_at AS BLOB)) <= 64))) IS NOT 1
BEGIN
    SELECT RAISE(ABORT, 'Replicated row values are invalid');
END;

CREATE TRIGGER sync_immutable_chat_provider_device_status
BEFORE UPDATE OF id, device_id, provider_family_id ON chat_provider_device_status
WHEN OLD.id IS NOT NEW.id COLLATE BINARY
    OR ((OLD.device_id IS NOT NEW.device_id COLLATE BINARY OR OLD.provider_family_id IS NOT NEW.provider_family_id COLLATE BINARY) AND (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0)
BEGIN
    SELECT RAISE(ABORT, 'Replicated row keys and creation times cannot change');
END;

CREATE TRIGGER sync_recreate_chat_provider_device_status
BEFORE INSERT ON chat_provider_device_status
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = 402 AND row_key = NEW.id)
    AND NOT EXISTS (SELECT 1 FROM chat_provider_device_status WHERE id = NEW.id)
BEGIN
    SELECT RAISE(ABORT, 'A published row key cannot be created again');
END;

CREATE TRIGGER sync_capture_chat_provider_device_status_insert
AFTER INSERT ON chat_provider_device_status
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (402, NEW.id, 15, 1, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET
        mask = mask | excluded.mask, created = 1, deleted = 0;
END;

CREATE TRIGGER sync_capture_chat_provider_device_status_update
AFTER UPDATE ON chat_provider_device_status
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    SELECT 402, NEW.id, changed.mask, 0, 0, CAST(unixepoch('subsec') * 1000 AS INTEGER)
    FROM (
        SELECT
            (CASE WHEN OLD.available IS NOT NEW.available OR OLD.status_changed_at IS NOT NEW.status_changed_at COLLATE BINARY
                THEN 2 ELSE 0 END)
            | (CASE WHEN OLD.catalog_data IS NOT NEW.catalog_data COLLATE BINARY OR OLD.catalog_changed_at IS NOT NEW.catalog_changed_at COLLATE BINARY
                THEN 4 ELSE 0 END)
            | (CASE WHEN OLD.limits_data IS NOT NEW.limits_data COLLATE BINARY OR OLD.limits_observed_at IS NOT NEW.limits_observed_at COLLATE BINARY
                THEN 8 ELSE 0 END)
            AS mask
    ) AS changed
    WHERE changed.mask <> 0
    ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask;
END;

CREATE TRIGGER sync_capture_chat_provider_device_status_delete
AFTER DELETE ON chat_provider_device_status
WHEN (SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0
BEGIN
    INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)
    VALUES (402, OLD.id, 0, 0, 1, CAST(unixepoch('subsec') * 1000 AS INTEGER))
    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;
END;

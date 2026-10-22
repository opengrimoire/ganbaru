-- The baseline CHECKs written as NOT LIKE '%\\%' match only a doubled backslash, because SQLite
-- LIKE has no escape character by default. These triggers reject any backslash, a bare '..', and
-- a trailing '/..' segment in managed and review paths. Existing rows are left untouched; Rust
-- validates every path again before using it.

CREATE TRIGGER IF NOT EXISTS project_working_folders_managed_relative_path_shape_insert
BEFORE INSERT ON project_working_folders
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER IF NOT EXISTS project_working_folders_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON project_working_folders
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER IF NOT EXISTS chat_review_comments_relative_path_shape_insert
BEFORE INSERT ON chat_review_comments
WHEN instr(NEW.relative_path, char(92)) > 0
    OR NEW.relative_path = '..'
    OR NEW.relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER IF NOT EXISTS chat_review_comments_relative_path_shape_update
BEFORE UPDATE OF relative_path ON chat_review_comments
WHEN instr(NEW.relative_path, char(92)) > 0
    OR NEW.relative_path = '..'
    OR NEW.relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER IF NOT EXISTS chat_attachments_managed_relative_path_shape_insert
BEFORE INSERT ON chat_attachments
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER IF NOT EXISTS chat_attachments_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON chat_attachments
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER IF NOT EXISTS chat_resources_managed_relative_path_shape_insert
BEFORE INSERT ON chat_resources
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

CREATE TRIGGER IF NOT EXISTS chat_resources_managed_relative_path_shape_update
BEFORE UPDATE OF managed_relative_path ON chat_resources
WHEN instr(NEW.managed_relative_path, char(92)) > 0
    OR NEW.managed_relative_path = '..'
    OR NEW.managed_relative_path LIKE '%/..'
BEGIN
    SELECT RAISE(ABORT, 'Stored relative paths cannot contain a backslash or a parent segment');
END;

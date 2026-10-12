-- Projects custom fields: an optional chosen Lucide icon; NULL shows the field type icon.

ALTER TABLE project_custom_fields
    ADD COLUMN icon TEXT CHECK (icon IS NULL OR icon GLOB 'lucide:?*');

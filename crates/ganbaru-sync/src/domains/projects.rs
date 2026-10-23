//! Projects value rules shared by the manifest and the engine: column bounds, the managed icon
//! file reference, and the adapter checking values the column types cannot express.
//!
//! A managed icon file is stored as `project-icons/{sha256}.{ext}`, the class name of
//! [`ICON_CLASS`] followed by the engine's asset file name. Group and project icons prefix it with
//! `asset:` and custom emojis store it bare, so a reference names exactly one content-addressed
//! file and never an arbitrary path.

use std::fmt;

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};
use sqlx::SqliteConnection;

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::assets::{self, AssetRef};
use crate::codec;
use crate::hooks::{ChangeSink, HookFuture, Hooks, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::projects::{
    ADAPTER, CHANGE_EVENTS_TABLE, CHECKLIST_TABLE, DEPENDENCIES_TABLE, DOMAIN, EMOJIS_TABLE,
    FIELDS_TABLE, FOLDERS_TABLE, GROUPS_TABLE, ICON_CLASS, OPTION_VALUES, PRIORITIES_TABLE,
    PROJECTS_TABLE, STATUSES_TABLE, TAG_LINKS_TABLE, TASKS_TABLE, VALUES_TABLE,
    VIEW_PREFERENCES_TABLE, change_event_group, dependency_group, emoji_group, field_group,
    folder_group, group_group, priority_group, project_group, status_group, tag_link_group,
    task_group, value_group, view_preference_group,
};

/// Characters in an enumerated column.
pub const MAX_ENUM_CHARS: usize = 32;

/// Characters in an opaque id held as a value, such as a priority id or a playlist id.
pub const MAX_ID_CHARS: usize = 128;

/// Characters in a name.
pub const MAX_NAME_CHARS: usize = 1_000;

/// Characters in an icon reference or emoji asset path.
pub const MAX_ICON_CHARS: usize = 500;

/// Characters in a task or checklist title.
pub const MAX_TITLE_CHARS: usize = 2_000;

/// Characters in a description, blocker reason, custom text value, or change event text.
pub const MAX_LONG_TEXT_CHARS: usize = 65_536;

/// Characters in a view preference value, the command bound.
pub const MAX_PREFERENCE_CHARS: usize = 20_000;

/// Characters in a stored date.
pub const MAX_DATE_CHARS: usize = 32;

/// Characters in a stored `HH:MM` time.
pub const MAX_TIME_CHARS: usize = 5;

/// Characters in a working folder display name, the schema bound.
pub const MAX_FOLDER_NAME_CHARS: usize = 240;

/// Characters in a working folder id, the schema bound.
pub const MAX_FOLDER_ID_CHARS: usize = 1_024;

/// Characters in a managed working folder path, the schema bound.
pub const MAX_RELATIVE_PATH_CHARS: usize = 2_048;

/// Characters in a repository identity, the schema bound.
pub const MAX_REPOSITORY_IDENTITY_CHARS: usize = 1_024;

/// Bytes in a project icon or custom emoji file, the asset class bound.
pub const MAX_ICON_FILE_BYTES: u32 = 3 * 1024 * 1024;

/// Bytes in a project icon or custom emoji file, the schema bound.
pub const MAX_ICON_BYTES: i64 = MAX_ICON_FILE_BYTES as i64;

/// Selected options in one multi-select value.
pub const MAX_SELECTED_OPTIONS: usize = 1_000;

/// Prefix of a group or project icon that references a managed icon file.
pub const ICON_REFERENCE_PREFIX: &str = "asset:";

/// Content types a project icon or custom emoji file may have.
pub const ICON_CONTENT_TYPES: [&str; 3] = ["image/png", "image/jpeg", "image/webp"];

/// A path that is not a managed project icon file, or that cannot name an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidIconPath;

impl fmt::Display for InvalidIconPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "project icon must be a managed {} file of a supported image",
            ICON_CLASS.name
        )
    }
}

impl std::error::Error for InvalidIconPath {}

/// The file a managed project icon path names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IconFile {
    /// Lowercase hex SHA-256 of the file bytes.
    pub sha256: String,
    /// One of [`ICON_CONTENT_TYPES`].
    pub content_type: String,
}

impl IconFile {
    /// The asset this file is, given its exact size.
    pub fn asset(&self, byte_size: u32) -> Result<AssetRef, InvalidIconPath> {
        AssetRef::new(
            &ICON_CLASS,
            self.sha256.clone(),
            self.content_type.clone(),
            byte_size,
        )
        .map_err(|_| InvalidIconPath)
    }

    /// The stored path, `project-icons/{sha256}.{ext}`.
    pub fn path(&self) -> Result<String, InvalidIconPath> {
        icon_path(&self.sha256, &self.content_type)
    }
}

/// The path stored for an icon file, after checking the digest and content type.
pub fn icon_path(sha256: &str, content_type: &str) -> Result<String, InvalidIconPath> {
    if !ICON_CONTENT_TYPES.contains(&content_type) {
        return Err(InvalidIconPath);
    }
    let name = assets::file_name(sha256, content_type).map_err(|_| InvalidIconPath)?;
    Ok(format!("{}/{name}", ICON_CLASS.name))
}

/// The file a stored icon path names, such as a custom emoji `asset_path`. Accepts only the exact
/// form [`icon_path`] produces, so no other spelling, directory, or extension reaches the
/// filesystem or the asset store.
pub fn parse_icon_path(path: &str) -> Result<IconFile, InvalidIconPath> {
    let name = path
        .strip_prefix(ICON_CLASS.name)
        .and_then(|rest| rest.strip_prefix('/'))
        .ok_or(InvalidIconPath)?;
    let (sha256, extension) = name.split_once('.').ok_or(InvalidIconPath)?;
    let content_type = ICON_CONTENT_TYPES
        .iter()
        .find(|content_type| assets::extension(content_type) == extension)
        .ok_or(InvalidIconPath)?;
    let file = IconFile {
        sha256: sha256.to_string(),
        content_type: (*content_type).to_string(),
    };
    if file.path()? != path {
        return Err(InvalidIconPath);
    }
    Ok(file)
}

/// The file a group or project icon names: `None` for an icon that is not a managed file
/// reference, and an error for an `asset:` reference that is not exactly a managed icon path.
pub fn parse_icon_reference(icon: &str) -> Result<Option<IconFile>, InvalidIconPath> {
    icon.strip_prefix(ICON_REFERENCE_PREFIX)
        .map(parse_icon_path)
        .transpose()
}

/// Prefix of a custom field icon.
const FIELD_ICON_PREFIX: &str = "lucide:";

/// Values the schema allows for each enumerated Projects column.
mod allowed {
    pub const PROJECT_STATUSES: &[&str] = &["active", "hidden", "archived"];
    pub const EVENT_TIME_MODES: &[&str] = &["timed", "all_day"];
    pub const POMODORO_MODES: &[&str] = &["none", "preset", "custom"];
    pub const POMODORO_PRESETS: &[&str] = &["adaptive", "creative", "balanced", "deep", "extended"];
    pub const IDLE_SOURCES: &[&str] = &["global", "custom"];
    pub const IDLE_THRESHOLDS: &[i64] = &[1, 2, 3, 4, 5, 10, 15];
    pub const NOTES_OPEN_MODES: &[&str] = &["center", "side", "full"];
    pub const NOTES_RETENTION_DAYS: &[i64] = &[0, 7, 30, 90, 180, 365];
    pub const FOLDER_KINDS: &[&str] = &["managed", "external"];
    pub const REPOSITORY_KINDS: &[&str] = &["git", "none"];
    pub const STATUS_CATEGORIES: &[&str] = &["not_started", "active", "blocked", "done"];
    pub const TASK_TYPES: &[&str] = &["task", "milestone", "bug", "habit"];
    pub const DEPENDENCY_TYPES: &[&str] = &["blocks"];
    pub const FIELD_TYPES: &[&str] = &[
        "text",
        "number",
        "select",
        "multi_select",
        "status",
        "date",
        "person",
        "files",
        "checkbox",
        "url",
        "phone",
        "email",
    ];
    pub const CHANGE_EVENT_TYPES: &[&str] = &[
        "created",
        "updated",
        "status_changed",
        "scheduled",
        "completed",
        "reopened",
        "archived",
        "event_unlinked",
        "dependency_added",
        "dependency_removed",
    ];
}

/// Rules of Projects values the column types cannot express: enumerations, the multi-column
/// CHECKs inside one group, required references, icon and path shapes, and time formats.
pub struct ProjectsAdapter;

fn one_of(field: &Field, values: &[&str], what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Text(text) if values.contains(&text.as_str()) => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

fn optional_one_of(field: &Field, values: &[&str], what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        field => one_of(field, values, what),
    }
}

fn optional_integer_in(
    field: &Field,
    values: &[i64],
    what: &'static str,
) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        Field::Integer(value) if values.contains(value) => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

/// A row key the schema declares `NOT NULL`.
fn required(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Text(_) => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

/// Null or text with a non-space character, the schema's `trim(column) <> ''`.
fn optional_non_blank(field: &Field, what: &'static str) -> Result<(), AdapterError> {
    match field {
        Field::Null => Ok(()),
        Field::Text(text) if !text.trim_matches(' ').is_empty() => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

fn fields<'value, const N: usize>(
    value: &'value Value,
    what: &'static str,
) -> Result<&'value [Field; N], AdapterError> {
    value.fields().try_into().map_err(|_| AdapterError(what))
}

/// Checks the row keys of an identity group the schema declares `NOT NULL`.
fn required_identity(value: &Value, what: &'static str) -> Result<(), AdapterError> {
    value.fields().iter().try_for_each(|field| match field {
        Field::Null => Err(AdapterError(what)),
        _ => Ok(()),
    })
}

/// The managed file asset a group or project icon value names, or `None` for an icon that names
/// no file. A size belongs only to a managed icon reference, and a reference needs its size.
fn icon_asset(value: &Value, what: &'static str) -> Result<Option<AssetRef>, AdapterError> {
    let (icon, size) = match fields(value, what)? {
        [Field::Text(icon), Field::Null] => (icon, None),
        [Field::Text(icon), Field::Integer(size)] => (icon, Some(*size)),
        _ => return Err(AdapterError(what)),
    };
    match (parse_icon_reference(icon), size) {
        (Ok(None), None) => Ok(None),
        (Ok(Some(file)), Some(size)) => sized_icon_asset(&file, size, what).map(Some),
        _ => Err(AdapterError(what)),
    }
}

/// The asset of a managed icon file with its stored size.
fn sized_icon_asset(
    file: &IconFile,
    size: i64,
    what: &'static str,
) -> Result<AssetRef, AdapterError> {
    let size = u32::try_from(size).map_err(|_| AdapterError(what))?;
    file.asset(size).map_err(|_| AdapterError(what))
}

/// The managed file asset of a custom emoji asset group, one file directly under the project
/// icon folder with its size.
fn emoji_asset(value: &Value) -> Result<AssetRef, AdapterError> {
    const WHAT: &str = "project emoji asset";
    match fields(value, WHAT)? {
        [Field::Text(path), Field::Integer(size)] => {
            let file = parse_icon_path(path).map_err(|_| AdapterError(WHAT))?;
            sized_icon_asset(&file, *size, WHAT)
        }
        _ => Err(AdapterError(WHAT)),
    }
}

/// A `HH:MM` time on a 24 hour clock.
fn is_clock_time(time: &str) -> bool {
    let bytes = time.as_bytes();
    let [hour_tens, hour_units, b':', minute_tens, minute_units] = bytes else {
        return false;
    };
    if ![hour_tens, hour_units, minute_tens, minute_units]
        .iter()
        .all(|digit| digit.is_ascii_digit())
    {
        return false;
    }
    let hour = (hour_tens - b'0') * 10 + (hour_units - b'0');
    let minute = (minute_tens - b'0') * 10 + (minute_units - b'0');
    hour <= 23 && minute <= 59
}

/// The task schedule CHECKs: a time or reminder needs its date, and a target end date does not
/// precede the start date.
fn validate_schedule(value: &Value) -> Result<(), AdapterError> {
    let [
        start_date,
        start_time,
        start_reminder,
        due_date,
        due_time,
        due_reminder,
        target_end,
    ] = fields(value, "project task schedule")?;
    for (date, time, reminder) in [
        (start_date, start_time, start_reminder),
        (due_date, due_time, due_reminder),
    ] {
        let has_date = matches!(date, Field::Text(_));
        match time {
            Field::Null => {}
            Field::Text(time) if has_date && is_clock_time(time) => {}
            _ => return Err(AdapterError("project task time")),
        }
        if !has_date && !matches!(reminder, Field::Null) {
            return Err(AdapterError("project task reminder"));
        }
    }
    match (start_date, target_end) {
        (Field::Text(start), Field::Text(end)) if end < start => {
            Err(AdapterError("project task target end date"))
        }
        _ => Ok(()),
    }
}

/// The project default Focus CHECK: no durations without a mode that uses them.
fn validate_pomodoro_defaults(value: &Value) -> Result<(), AdapterError> {
    let [mode, preset, focus, short_break, long_break, count] =
        fields(value, "project pomodoro defaults")?;
    one_of(mode, allowed::POMODORO_MODES, "project pomodoro mode")?;
    optional_one_of(preset, allowed::POMODORO_PRESETS, "project pomodoro preset")?;
    let durations = [focus, short_break, long_break, count];
    let none = |fields: &[&Field]| fields.iter().all(|field| matches!(field, Field::Null));
    let all = |fields: &[&Field]| fields.iter().all(|field| !matches!(field, Field::Null));
    let consistent = match mode {
        Field::Text(mode) if mode == "none" => none(&[preset]) && none(&durations),
        Field::Text(mode) if mode == "preset" => all(&[preset]) && none(&durations),
        Field::Text(mode) if mode == "custom" => none(&[preset]) && all(&durations),
        _ => false,
    };
    if consistent {
        Ok(())
    } else {
        Err(AdapterError("project pomodoro defaults"))
    }
}

fn validate_project(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == project_group::GROUP {
        let [group] = fields(value, "project group")?;
        required(group, "project group")
    } else if group == project_group::ICON {
        icon_asset(value, "project icon").map(|_| ())
    } else if group == project_group::STATUS {
        let [status] = fields(value, "project status")?;
        one_of(status, allowed::PROJECT_STATUSES, "project status")
    } else if group == project_group::EVENT_DEFAULTS {
        let [_, name, time_mode] = fields(value, "project event defaults")?;
        optional_non_blank(name, "project default event name")?;
        one_of(
            time_mode,
            allowed::EVENT_TIME_MODES,
            "project default event time mode",
        )
    } else if group == project_group::POMODORO_DEFAULTS {
        validate_pomodoro_defaults(value)
    } else if group == project_group::IDLE_DEFAULTS {
        let [source, _, threshold] = fields(value, "project idle defaults")?;
        one_of(source, allowed::IDLE_SOURCES, "project idle source")?;
        optional_integer_in(
            threshold,
            allowed::IDLE_THRESHOLDS,
            "project idle threshold",
        )
    } else if group == project_group::NOTES_SETTINGS {
        let [mode, retention] = fields(value, "project notes settings")?;
        optional_one_of(mode, allowed::NOTES_OPEN_MODES, "project notes open mode")?;
        optional_integer_in(
            retention,
            allowed::NOTES_RETENTION_DAYS,
            "project notes retention",
        )
    } else {
        Ok(())
    }
}

/// A managed relative path stays inside the project folder: relative, without a parent segment
/// or a backslash.
fn is_contained_relative_path(path: &str) -> bool {
    !path.is_empty()
        && !path.starts_with('/')
        && !path.contains('\\')
        && path.split('/').all(|segment| segment != "..")
}

fn validate_folder(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == folder_group::CREATED {
        let [project, kind, path, _, _] = fields(value, "project folder")?;
        required(project, "project folder project")?;
        one_of(kind, allowed::FOLDER_KINDS, "project folder kind")?;
        match (kind, path) {
            (Field::Text(kind), Field::Text(path))
                if kind == "managed" && is_contained_relative_path(path) =>
            {
                Ok(())
            }
            (Field::Text(kind), Field::Null) if kind == "external" => Ok(()),
            _ => Err(AdapterError("project folder path")),
        }
    } else if group == folder_group::REPOSITORY {
        let [kind, identity] = fields(value, "project folder repository")?;
        one_of(
            kind,
            allowed::REPOSITORY_KINDS,
            "project folder repository kind",
        )?;
        match (kind, identity) {
            (Field::Text(kind), Field::Text(identity)) if kind == "git" && !identity.is_empty() => {
                Ok(())
            }
            (Field::Text(kind), Field::Null) if kind == "none" => Ok(()),
            _ => Err(AdapterError("project folder repository")),
        }
    } else if group == folder_group::ARCHIVED {
        match fields(value, "project folder archive")? {
            [Field::Null] => Ok(()),
            [Field::Text(archived)] if archived.len() >= 20 => Ok(()),
            _ => Err(AdapterError("project folder archive")),
        }
    } else {
        Ok(())
    }
}

fn validate_task(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == task_group::SECTION_PLACE {
        let [section, _] = fields(value, "project task section")?;
        required(section, "project task section")
    } else if group == task_group::STATUS_PLACE {
        let [status, _, _] = fields(value, "project task status")?;
        required(status, "project task status")
    } else if group == task_group::KIND {
        let [task_type, _] = fields(value, "project task type")?;
        one_of(task_type, allowed::TASK_TYPES, "project task type")
    } else if group == task_group::SCHEDULE {
        validate_schedule(value)
    } else {
        Ok(())
    }
}

/// A custom field value holds at most one scalar kind.
fn validate_field_value(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group == value_group::IDENTITY {
        return required_identity(value, "project custom field value identity");
    }
    if group != value_group::VALUE {
        return Ok(());
    }
    let [text, number, date, checkbox, Field::Blob(options)] =
        fields(value, "project custom field value")?
    else {
        return Err(AdapterError("project custom field value"));
    };
    let present = [text, number, date, checkbox]
        .iter()
        .filter(|field| !matches!(field, Field::Null))
        .count();
    if present > 1 {
        return Err(AdapterError("project custom field value"));
    }
    match checkbox {
        Field::Null | Field::Integer(0 | 1) => {}
        _ => return Err(AdapterError("project custom field checkbox")),
    }
    let rows = codec::decode(&OPTION_VALUES, options)
        .map_err(|_| AdapterError("project custom field options"))?;
    if rows
        .iter()
        .all(|row| matches!(row.fields.as_slice(), [Field::Text(_)]))
    {
        Ok(())
    } else {
        Err(AdapterError("project custom field option"))
    }
}

/// A custom emoji asset is one managed file directly under the project icon folder.
fn validate_emoji(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    if group != emoji_group::ASSET {
        return Ok(());
    }
    emoji_asset(value).map(|_| ())
}

impl DomainAdapter for ProjectsAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn asset_groups(&self, table: TableId) -> &'static [GroupId] {
        match table {
            GROUPS_TABLE => &[group_group::ICON],
            PROJECTS_TABLE => &[project_group::ICON],
            EMOJIS_TABLE => &[emoji_group::ASSET],
            _ => &[],
        }
    }

    fn asset_refs(
        &self,
        table: TableId,
        group: GroupId,
        value: &Value,
    ) -> Result<Vec<AssetRef>, AdapterError> {
        let asset = match table {
            GROUPS_TABLE if group == group_group::ICON => icon_asset(value, "project group icon")?,
            PROJECTS_TABLE if group == project_group::ICON => icon_asset(value, "project icon")?,
            EMOJIS_TABLE if group == emoji_group::ASSET => Some(emoji_asset(value)?),
            _ => None,
        };
        Ok(asset.into_iter().collect())
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    fn priority(&self, _table: TableId, _group: GroupId, _value: &Value) -> u8 {
        0
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        match table {
            GROUPS_TABLE if group == group_group::ICON => {
                icon_asset(value, "project group icon").map(|_| ())
            }
            PROJECTS_TABLE => validate_project(group, value),
            FOLDERS_TABLE => validate_folder(group, value),
            STATUSES_TABLE if group == status_group::CATEGORY => {
                let [category] = fields(value, "project status category")?;
                one_of(
                    category,
                    allowed::STATUS_CATEGORIES,
                    "project status category",
                )
            }
            PRIORITIES_TABLE if group == priority_group::IDENTITY => {
                required_identity(value, "project priority identity")
            }
            TASKS_TABLE => validate_task(group, value),
            CHECKLIST_TABLE => Ok(()),
            DEPENDENCIES_TABLE if group == dependency_group::IDENTITY => {
                let [blocking, blocked] = fields(value, "project dependency")?;
                required(blocking, "project dependency blocking task")?;
                required(blocked, "project dependency blocked task")?;
                if blocking == blocked {
                    Err(AdapterError("project dependency on itself"))
                } else {
                    Ok(())
                }
            }
            DEPENDENCIES_TABLE if group == dependency_group::KIND => {
                let [kind] = fields(value, "project dependency type")?;
                one_of(kind, allowed::DEPENDENCY_TYPES, "project dependency type")
            }
            TAG_LINKS_TABLE if group == tag_link_group::IDENTITY => {
                required_identity(value, "project task tag identity")
            }
            FIELDS_TABLE if group == field_group::CREATED => {
                let [project, field_type, _] = fields(value, "project custom field")?;
                required(project, "project custom field project")?;
                one_of(
                    field_type,
                    allowed::FIELD_TYPES,
                    "project custom field type",
                )
            }
            FIELDS_TABLE if group == field_group::ICON => match fields(value, "field icon")? {
                [Field::Null] => Ok(()),
                [Field::Text(icon)]
                    if icon.len() > FIELD_ICON_PREFIX.len()
                        && icon.starts_with(FIELD_ICON_PREFIX) =>
                {
                    Ok(())
                }
                _ => Err(AdapterError("project custom field icon")),
            },
            VALUES_TABLE => validate_field_value(group, value),
            CHANGE_EVENTS_TABLE if group == change_event_group::CREATED => {
                let [task, event_type, ..] = value.fields() else {
                    return Err(AdapterError("project task change event"));
                };
                required(task, "project task change event task")?;
                one_of(
                    event_type,
                    allowed::CHANGE_EVENT_TYPES,
                    "project task change event type",
                )
            }
            VIEW_PREFERENCES_TABLE if group == view_preference_group::IDENTITY => {
                required_identity(value, "project view preference identity")
            }
            EMOJIS_TABLE => validate_emoji(group, value),
            _ => Ok(()),
        }
    }

    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let mut presentation = Presentation::default();
        let title_group = match table {
            PROJECTS_TABLE => project_group::NAME,
            TASKS_TABLE => task_group::TITLE,
            _ => return presentation,
        };
        for (group, value) in values {
            if *group == title_group
                && let [Field::Text(title)] = value.fields()
            {
                presentation.title = title.trim_matches(' ').to_string();
            }
        }
        presentation
    }
}

/// Reports every Projects row a materialization changed, with its table name as the topic and
/// its row key as the key, so the app refreshes open Projects views and task reminders. It
/// rebuilds nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProjectsHook;

impl MaterializeHook for ProjectsHook {
    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            for key in changed_keys {
                changes.report(table.name, key.clone());
            }
            Ok(())
        })
    }
}

/// `hooks` with [`ProjectsHook`] registered for every Projects table.
pub fn with_hooks(hooks: Hooks) -> Hooks {
    DOMAIN.tables.iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(ProjectsHook))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(value: &str) -> Field {
        Field::Text(value.to_string())
    }

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    fn schedule(fields: [Field; 7]) -> Value {
        value(fields.to_vec())
    }

    #[test]
    fn clock_times_accept_only_a_24_hour_minute() {
        assert!(is_clock_time("00:00"));
        assert!(is_clock_time("23:59"));
        assert!(!is_clock_time("24:00"));
        assert!(!is_clock_time("12:60"));
        assert!(!is_clock_time("1:30"));
        assert!(!is_clock_time("12-30"));
    }

    #[test]
    fn schedules_need_a_date_for_each_time_and_reminder() {
        let dated = schedule([
            text("2026-10-01"),
            text("09:30"),
            Field::Integer(15),
            Field::Null,
            Field::Null,
            Field::Null,
            text("2026-10-02"),
        ]);
        assert!(validate_schedule(&dated).is_ok());
        let undated_time = schedule([
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
            text("09:30"),
            Field::Null,
            Field::Null,
        ]);
        assert!(validate_schedule(&undated_time).is_err());
        let undated_reminder = schedule([
            Field::Null,
            Field::Null,
            Field::Integer(5),
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
        ]);
        assert!(validate_schedule(&undated_reminder).is_err());
        let reversed = schedule([
            text("2026-10-02"),
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
            text("2026-10-01"),
        ]);
        assert!(validate_schedule(&reversed).is_err());
    }

    #[test]
    fn pomodoro_defaults_follow_their_mode() {
        let preset = value(vec![
            text("preset"),
            text("deep"),
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
        ]);
        assert!(validate_pomodoro_defaults(&preset).is_ok());
        let custom = value(vec![
            text("custom"),
            Field::Null,
            Field::Integer(50),
            Field::Integer(10),
            Field::Integer(20),
            Field::Integer(3),
        ]);
        assert!(validate_pomodoro_defaults(&custom).is_ok());
        let partial = value(vec![
            text("custom"),
            Field::Null,
            Field::Integer(50),
            Field::Null,
            Field::Integer(20),
            Field::Integer(3),
        ]);
        assert!(validate_pomodoro_defaults(&partial).is_err());
        let stray_preset = value(vec![
            text("none"),
            text("deep"),
            Field::Null,
            Field::Null,
            Field::Null,
            Field::Null,
        ]);
        assert!(validate_pomodoro_defaults(&stray_preset).is_err());
    }

    #[test]
    fn managed_paths_stay_inside_the_project_folder() {
        assert!(is_contained_relative_path("projects/abc"));
        assert!(is_contained_relative_path("projects/a..b"));
        assert!(!is_contained_relative_path("/projects/abc"));
        assert!(!is_contained_relative_path("projects/../abc"));
        assert!(!is_contained_relative_path(".."));
        assert!(!is_contained_relative_path("projects\\abc"));
        assert!(!is_contained_relative_path(""));
    }

    #[test]
    fn custom_field_values_hold_at_most_one_scalar() {
        let adapter = ProjectsAdapter;
        let blob = Field::Blob(codec::encode(&OPTION_VALUES, &[]).unwrap());
        let single = value(vec![
            text("note"),
            Field::Null,
            Field::Null,
            Field::Null,
            blob.clone(),
        ]);
        assert!(
            adapter
                .validate(VALUES_TABLE, value_group::VALUE, &single)
                .is_ok()
        );
        let double = value(vec![
            text("note"),
            Field::Integer(3),
            Field::Null,
            Field::Null,
            blob,
        ]);
        assert!(
            adapter
                .validate(VALUES_TABLE, value_group::VALUE, &double)
                .is_err()
        );
    }

    #[test]
    fn icon_sizes_belong_only_to_managed_icons() {
        let adapter = ProjectsAdapter;
        let managed = value(vec![
            text(&format!("asset:project-icons/{DIGEST}.png")),
            Field::Integer(10),
        ]);
        assert!(
            adapter
                .validate(PROJECTS_TABLE, project_group::ICON, &managed)
                .is_ok()
        );
        for invalid in [
            value(vec![text("folder"), Field::Integer(10)]),
            value(vec![
                text(&format!("asset:project-icons/{DIGEST}.png")),
                Field::Null,
            ]),
        ] {
            assert!(
                adapter
                    .validate(PROJECTS_TABLE, project_group::ICON, &invalid)
                    .is_err()
            );
        }
    }

    const DIGEST: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    #[test]
    fn icon_paths_round_trip_every_content_type() {
        for content_type in ICON_CONTENT_TYPES {
            let path = icon_path(DIGEST, content_type).unwrap();
            let file = parse_icon_path(&path).unwrap();
            assert_eq!(file.sha256, DIGEST);
            assert_eq!(file.content_type, content_type);
            assert_eq!(
                parse_icon_reference(&format!("asset:{path}")).unwrap(),
                Some(file)
            );
        }
        assert!(icon_path(DIGEST, "image/gif").is_err());
    }

    #[test]
    fn icon_paths_refuse_foreign_and_non_canonical_paths() {
        for path in [
            format!("project-icons/{}.png", DIGEST.to_uppercase()),
            format!("project-icons/{DIGEST}.jpeg"),
            format!("project-icons/{DIGEST}.gif"),
            format!("project-icons/{DIGEST}.svg"),
            format!("project-icons/nested/{DIGEST}.png"),
            format!("project-icons/../{DIGEST}.png"),
            format!("music/artwork/{DIGEST}.png"),
            format!("/project-icons/{DIGEST}.png"),
            format!(" project-icons/{DIGEST}.png"),
            "project-icons/abcdef.png".to_string(),
            "project-icons/".to_string(),
        ] {
            assert!(parse_icon_path(&path).is_err(), "{path} parsed");
        }
        assert_eq!(parse_icon_reference("lucide:folder"), Ok(None));
        assert_eq!(parse_icon_reference("none"), Ok(None));
        assert!(parse_icon_reference("asset:quick-notes/images/a.png").is_err());
    }

    #[test]
    fn asset_refs_parse_each_icon_column_form() {
        let adapter = ProjectsAdapter;
        let reference = |path: &str| text(&format!("asset:{path}"));
        let path = icon_path(DIGEST, "image/webp").unwrap();
        for (table, group) in [
            (GROUPS_TABLE, group_group::ICON),
            (PROJECTS_TABLE, project_group::ICON),
        ] {
            assert_eq!(adapter.asset_groups(table), &[group]);
            let refs = adapter
                .asset_refs(
                    table,
                    group,
                    &value(vec![reference(&path), Field::Integer(42)]),
                )
                .unwrap();
            assert_eq!(refs.len(), 1);
            assert_eq!(refs[0].class(), &ICON_CLASS);
            assert_eq!(refs[0].sha256(), DIGEST);
            assert_eq!(refs[0].content_type(), "image/webp");
            assert_eq!(refs[0].byte_size(), 42);
            let named = value(vec![text("lucide:folder"), Field::Null]);
            assert!(adapter.asset_refs(table, group, &named).unwrap().is_empty());
            let foreign = value(vec![
                reference(&format!("music/artwork/{DIGEST}.png")),
                Field::Integer(42),
            ]);
            assert!(adapter.asset_refs(table, group, &foreign).is_err());
            assert!(adapter.validate(table, group, &foreign).is_err());
        }

        assert_eq!(adapter.asset_groups(EMOJIS_TABLE), &[emoji_group::ASSET]);
        let emoji = value(vec![text(&path), Field::Integer(7)]);
        let refs = adapter
            .asset_refs(EMOJIS_TABLE, emoji_group::ASSET, &emoji)
            .unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].byte_size(), 7);
        for invalid in [
            value(vec![text(&format!("asset:{path}")), Field::Integer(7)]),
            value(vec![text("project-icons/../escape.png"), Field::Integer(7)]),
            value(vec![text(&path), Field::Null]),
        ] {
            assert!(
                adapter
                    .asset_refs(EMOJIS_TABLE, emoji_group::ASSET, &invalid)
                    .is_err()
            );
            assert!(
                adapter
                    .validate(EMOJIS_TABLE, emoji_group::ASSET, &invalid)
                    .is_err()
            );
        }
        assert!(adapter.asset_groups(TASKS_TABLE).is_empty());
    }

    #[test]
    fn icon_assets_respect_the_class_bound() {
        let adapter = ProjectsAdapter;
        let path = icon_path(DIGEST, "image/png").unwrap();
        let reference = text(&format!("asset:{path}"));
        let largest = value(vec![
            reference.clone(),
            Field::Integer(i64::from(ICON_CLASS.max_bytes)),
        ]);
        assert!(
            adapter
                .asset_refs(PROJECTS_TABLE, project_group::ICON, &largest)
                .is_ok()
        );
        for size in [0, i64::from(ICON_CLASS.max_bytes) + 1, -1] {
            let invalid = value(vec![reference.clone(), Field::Integer(size)]);
            assert!(
                adapter
                    .asset_refs(PROJECTS_TABLE, project_group::ICON, &invalid)
                    .is_err()
            );
        }
    }

    #[test]
    fn dependencies_never_block_themselves() {
        let adapter = ProjectsAdapter;
        let edge = value(vec![text("a"), text("a")]);
        assert!(
            adapter
                .validate(DEPENDENCIES_TABLE, dependency_group::IDENTITY, &edge)
                .is_err()
        );
    }
}

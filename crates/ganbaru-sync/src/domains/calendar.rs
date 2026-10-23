//! Calendar value rules shared by the manifest and the engine: column bounds, the enumerations
//! the schema checks, the owned row codec, and the adapter checking values the column types
//! cannot express.
//!
//! Every Calendar owned table uses the generic row encoding through the adapter, which first
//! checks the enumerations and the key order the schema relies on. A keyed table's rows are in
//! strictly ascending key order, text compared bytewise and integers numerically, so a list
//! encodes only in the order materialize reads it back.

use std::cmp::Ordering;

use ganbaru_sync_contracts::bounds::{MAX_BLOB_FIELD_BYTES, MAX_TEXT_FIELD_BYTES};
use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::codec::{self, OwnedRow};
use crate::manifest::vault::calendar::{
    ADAPTER, ARCHIVE_OBJECTS_TABLE, ARCHIVES_TABLE, CALENDARS_TABLE, COMPONENTS_TABLE, DOMAIN,
    EVENTS_TABLE, OBJECTS_TABLE, TASK_EVENT_LINKS_TABLE, archive_group, archive_object_group,
    calendar_group, component_group, event_group, object_group, task_event_link_group,
};
use crate::manifest::{ColumnSpec, ColumnType, OwnedOrder, OwnedTable};

/// Characters in a free text column or an id held as data. The value field bound limits text
/// first, so the character bound adds nothing to it.
pub const MAX_TEXT_CHARS: usize = MAX_TEXT_FIELD_BYTES;

/// Characters in an enumerated column.
pub const MAX_ENUM_CHARS: usize = 32;

/// Rows of an owned list without a smaller natural bound: the codec's row count limit.
pub const MAX_OWNED_ROWS: usize = u16::MAX as usize;

/// Focus sequence steps per event, the schema bound.
pub const MAX_FOCUS_STEPS: usize = 12;

/// Archived music assignments per archive: two owner kinds by three phases.
pub const MAX_MUSIC_ASSIGNMENTS: usize = 6;

/// Stored geographic coordinates per degree.
pub const GEO_UNITS_PER_DEGREE: i64 = 1_000_000;

/// Largest stored latitude magnitude, 90 degrees.
pub const MAX_GEO_LATITUDE: i64 = 90 * GEO_UNITS_PER_DEGREE;

/// Largest stored longitude magnitude, 180 degrees.
pub const MAX_GEO_LONGITUDE: i64 = 180 * GEO_UNITS_PER_DEGREE;

/// Characters in an archived recurrence date, `YYYY-MM-DD`.
pub const RECURRENCE_DATE_CHARS: usize = 10;

/// Enumerations the schema checks, each sorted ascending.
pub mod allowed {
    /// `calendars.source`.
    pub const CALENDAR_SOURCES: &[&str] = &["ics", "local"];
    /// `icalendar_objects.source_kind`.
    pub const OBJECT_SOURCE_KINDS: &[&str] = &[
        "import-file",
        "import-zip-entry",
        "local-export-base",
        "subscription",
    ];
    /// `icalendar_components.preservation_status`.
    pub const PRESERVATION_STATUSES: &[&str] = &[
        "invalid",
        "lossless",
        "needs-review",
        "partial",
        "regenerated",
        "unsupported",
    ];
    /// `icalendar_value_nodes.value_kind`.
    pub const VALUE_KINDS: &[&str] = &["array", "boolean", "null", "number", "object", "text"];
    /// Event and override status.
    pub const EVENT_STATUSES: &[&str] = &["cancelled", "confirmed", "tentative"];
    /// Event and override transparency.
    pub const TRANSPARENCIES: &[&str] = &["opaque", "transparent"];
    /// Event and override visibility.
    pub const VISIBILITIES: &[&str] = &["private", "public"];
    /// Attendee participation status and the local RSVP status.
    pub const PARTICIPATION_STATUSES: &[&str] = &[
        "accepted",
        "declined",
        "delegated",
        "needs-action",
        "tentative",
    ];
    /// Attendee role.
    pub const ATTENDEE_ROLES: &[&str] = &[
        "chair",
        "non-participant",
        "opt-participant",
        "req-participant",
    ];
    /// Alarm action.
    pub const ALARM_ACTIONS: &[&str] = &["audio", "display", "email"];
    /// Alarm trigger type.
    pub const TRIGGER_TYPES: &[&str] = &["absolute", "relative"];
    /// Override recurrence range.
    pub const RECURRENCE_RANGES: &[&str] = &["this-and-future"];
    /// Focus rhythm kind.
    pub const RHYTHM_KINDS: &[&str] = &["count", "sequence"];
    /// Focus rhythm source.
    pub const RHYTHM_SOURCES: &[&str] = &["custom", "preset"];
    /// Focus preset.
    pub const PRESET_KEYS: &[&str] = &["adaptive", "balanced", "creative", "deep", "extended"];
    /// Focus sequence break phase.
    pub const BREAK_PHASES: &[&str] = &["long_break", "short_break"];
    /// Archived music assignment owner kind.
    pub const MUSIC_OWNER_KINDS: &[&str] = &["event-override", "event-snapshot"];
    /// Archived music assignment phase.
    pub const MUSIC_PHASES: &[&str] = &["focus", "long-break", "short-break"];
    /// Archived music behavior.
    pub const MUSIC_BEHAVIORS: &[&str] = &[
        "inherit",
        "keep-current-music",
        "pause-music",
        "play-automatically",
        "prepare-silently",
    ];
    /// Archived music provenance.
    pub const PROVENANCE_KINDS: &[&str] = &["copied-project", "explicit", "work-environment"];
    /// Archived soundscape behavior.
    pub const SOUNDSCAPE_BEHAVIORS: &[&str] = &[
        "inherit",
        "keep-current-soundscape",
        "pause-soundscape",
        "play-selected",
    ];
    /// Task event link kind.
    pub const TASK_LINK_KINDS: &[&str] = &["reference", "scheduled"];
}

/// The enumeration an owned table column holds, if any.
fn owned_literals(table: &str, column: &str) -> Option<&'static [&'static str]> {
    let literals = match column {
        "action" => allowed::ALARM_ACTIONS,
        "trigger_type" => allowed::TRIGGER_TYPES,
        "role" => allowed::ATTENDEE_ROLES,
        "status" if table.ends_with("attendees") => allowed::PARTICIPATION_STATUSES,
        "status" => allowed::EVENT_STATUSES,
        "transparency" => allowed::TRANSPARENCIES,
        "visibility" => allowed::VISIBILITIES,
        "recurrence_range" => allowed::RECURRENCE_RANGES,
        "rhythm_kind" => allowed::RHYTHM_KINDS,
        "rhythm_source" => allowed::RHYTHM_SOURCES,
        "preset_key" => allowed::PRESET_KEYS,
        "break_phase" => allowed::BREAK_PHASES,
        "owner_kind" => allowed::MUSIC_OWNER_KINDS,
        "phase" => allowed::MUSIC_PHASES,
        "behavior" => allowed::MUSIC_BEHAVIORS,
        "provenance_kind" => allowed::PROVENANCE_KINDS,
        "soundscape_behavior" => allowed::SOUNDSCAPE_BEHAVIORS,
        "link_kind" => allowed::TASK_LINK_KINDS,
        "value_kind" => allowed::VALUE_KINDS,
        _ => return None,
    };
    Some(literals)
}

/// Owned columns that are null or hold a non-space character.
const NON_BLANK_OPTIONAL: &[&str] = &["soundscape_preset_id"];

/// The owned table `owned` of a Calendar table.
fn owned_table(table: TableId, owned: &str) -> Result<&'static OwnedTable, AdapterError> {
    DOMAIN
        .tables
        .iter()
        .filter(|spec| spec.id == table)
        .flat_map(|spec| spec.groups.iter())
        .flat_map(|group| group.owned_tables().iter())
        .find(|candidate| candidate.table == owned)
        .ok_or(AdapterError("calendar owned table"))
}

/// Orders two key fields: text bytewise, integers numerically.
fn compare_key(left: &Field, right: &Field) -> Option<Ordering> {
    match (left, right) {
        (Field::Text(left), Field::Text(right)) => Some(left.as_bytes().cmp(right.as_bytes())),
        (Field::Integer(left), Field::Integer(right)) => Some(left.cmp(right)),
        _ => None,
    }
}

/// Positions of the key columns of a keyed table.
fn key_positions(owned: &OwnedTable) -> Vec<usize> {
    let OwnedOrder::Key(keys) = owned.order else {
        return Vec::new();
    };
    keys.iter()
        .filter_map(|key| owned.columns.iter().position(|column| column.name == *key))
        .collect()
}

/// Checks the rules of owned rows the column types cannot express: enumerations, non-blank
/// optional ids, and strictly ascending keys.
fn check_owned_rows(owned: &OwnedTable, rows: &[OwnedRow]) -> Result<(), AdapterError> {
    let invalid = AdapterError(owned.table);
    for row in rows {
        for (column, field) in owned.columns.iter().zip(&row.fields) {
            if let Some(literals) = owned_literals(owned.table, column.name) {
                match field {
                    Field::Null => {}
                    Field::Text(text) if literals.contains(&text.as_str()) => {}
                    _ => return Err(invalid),
                }
            }
            if NON_BLANK_OPTIONAL.contains(&column.name)
                && matches!(field, Field::Text(text) if text.trim_matches(' ').is_empty())
            {
                return Err(invalid);
            }
        }
    }
    let keys = key_positions(owned);
    if !keys.is_empty() {
        for pair in rows.windows(2) {
            let mut order = Ordering::Equal;
            for &position in &keys {
                let (Some(left), Some(right)) =
                    (pair[0].fields.get(position), pair[1].fields.get(position))
                else {
                    return Err(invalid);
                };
                order = compare_key(left, right).ok_or(invalid)?;
                if order != Ordering::Equal {
                    break;
                }
            }
            if order != Ordering::Less {
                return Err(invalid);
            }
        }
    }
    Ok(())
}

/// Encodes owned rows after checking them.
pub fn encode_rows(owned: &OwnedTable, rows: Vec<Vec<Field>>) -> Result<Vec<u8>, AdapterError> {
    let rows = rows.into_iter().map(OwnedRow::flat).collect::<Vec<_>>();
    check_owned_rows(owned, &rows)?;
    codec::encode(owned, &rows).map_err(AdapterError)
}

/// Decodes owned rows, refusing any list [`encode_rows`] refuses.
pub fn decode_rows(owned: &OwnedTable, bytes: &[u8]) -> Result<Vec<Vec<Field>>, AdapterError> {
    let rows = codec::decode(owned, bytes).map_err(AdapterError)?;
    check_owned_rows(owned, &rows)?;
    Ok(rows.into_iter().map(|row| row.fields).collect())
}

/// A timestamp every sample row uses.
const SAMPLE_TIMESTAMP: &str = "2026-01-01T00:00:00.000Z";

/// Digits of a sample serial, so serials sort bytewise in numeric order.
const SAMPLE_SERIAL_DIGITS: usize = 8;

/// Bytes kept free of long sample texts for the count, the field tags, and the short fields.
const SAMPLE_OVERHEAD_BYTES: usize = 4_096;

/// How a sample row fills its fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Fill {
    /// Long texts, upper integer bounds, and set optional columns.
    Long(usize),
    /// Short texts, lower integer bounds, and null optional columns.
    Short,
}

/// The key field of row `index` in a keyed column.
fn sample_key(
    owned: &OwnedTable,
    keys: &[&'static str],
    column: &ColumnSpec,
    index: usize,
) -> Field {
    // Literal keys count in mixed radix, the last key column fastest, so rows ascend.
    let literal_keys = keys
        .iter()
        .map(|key| owned_literals(owned.table, key))
        .collect::<Vec<_>>();
    if literal_keys.iter().all(Option::is_some) {
        let mut remainder = index;
        let mut field = Field::Null;
        for (key, literals) in keys.iter().zip(literal_keys).rev() {
            let literals = literals.unwrap_or_default();
            let digit = remainder % literals.len();
            remainder /= literals.len();
            if *key == column.name {
                field = Field::Text(literals[digit].to_owned());
            }
        }
        return field;
    }
    // Otherwise the first non-literal key counts and every other key column is constant.
    let counting = keys
        .iter()
        .find(|key| owned_literals(owned.table, key).is_none());
    if counting != Some(&column.name) {
        return match owned_literals(owned.table, column.name) {
            Some(literals) => Field::Text(literals[0].to_owned()),
            None => Field::Text("k".to_owned()),
        };
    }
    match column.ty {
        ColumnType::Integer { min, .. } => Field::Integer(min.saturating_add(index as i64)),
        _ => Field::Text(format!("{index:0SAMPLE_SERIAL_DIGITS$}")),
    }
}

/// A non-key field of a sample row.
fn sample_field(owned: &OwnedTable, column: &ColumnSpec, index: usize, fill: Fill) -> Field {
    let optional = matches!(
        column.ty,
        ColumnType::OptionalText { .. } | ColumnType::OptionalInteger { .. }
    );
    if fill == Fill::Short && optional {
        return Field::Null;
    }
    if let Some(literals) = owned_literals(owned.table, column.name) {
        // Short rows keep one literal so every short row encodes to the same length.
        let literal = match fill {
            Fill::Short => literals[0],
            Fill::Long(_) => literals[index % literals.len()],
        };
        return Field::Text(literal.to_owned());
    }
    match (column.ty, fill) {
        (
            ColumnType::Integer { min, .. } | ColumnType::OptionalInteger { min, .. },
            Fill::Short,
        ) => Field::Integer(min),
        (ColumnType::Integer { max, .. } | ColumnType::OptionalInteger { max, .. }, _) => {
            Field::Integer(max)
        }
        (ColumnType::Flag, Fill::Short) => Field::Integer(0),
        (ColumnType::Flag, _) => Field::Integer(1),
        (ColumnType::Timestamp | ColumnType::OptionalTimestamp, _) => {
            Field::Text(SAMPLE_TIMESTAMP.to_owned())
        }
        (ColumnType::Text { .. }, Fill::Short) => Field::Text(String::new()),
        (ColumnType::TrimmedText { .. }, Fill::Short) => Field::Text("x".to_owned()),
        (
            ColumnType::Text { max_chars }
            | ColumnType::OptionalText { max_chars }
            | ColumnType::TrimmedText { max_chars, .. },
            Fill::Long(chars),
        ) => Field::Text("x".repeat(chars.min(max_chars))),
        _ => Field::Null,
    }
}

/// `count` sample rows of `owned`.
fn sample_rows(owned: &OwnedTable, count: usize, fill: Fill) -> Vec<Vec<Field>> {
    let keys = match owned.order {
        OwnedOrder::Key(keys) => keys,
        _ => &[],
    };
    (0..count)
        .map(|index| {
            owned
                .columns
                .iter()
                .map(|column| {
                    if keys.contains(&column.name) {
                        sample_key(owned, keys, column, index)
                    } else {
                        sample_field(owned, column, index, fill)
                    }
                })
                .collect()
        })
        .collect()
}

/// Row lists that reach an owned table's bounds within one blob field: none, one row with the
/// longest texts that fit, and as many short rows as fit or the row limit allows.
pub fn owned_samples(owned: &OwnedTable) -> Vec<Vec<Vec<Field>>> {
    let keys = match owned.order {
        OwnedOrder::Key(keys) => keys,
        _ => &[],
    };
    let long_columns = owned
        .columns
        .iter()
        .filter(|column| {
            !keys.contains(&column.name)
                && owned_literals(owned.table, column.name).is_none()
                && matches!(
                    column.ty,
                    ColumnType::Text { .. }
                        | ColumnType::OptionalText { .. }
                        | ColumnType::TrimmedText { .. }
                )
        })
        .count()
        .max(1);
    let long_chars =
        ((MAX_BLOB_FIELD_BYTES - SAMPLE_OVERHEAD_BYTES) / long_columns).min(MAX_TEXT_FIELD_BYTES);
    let short_row = sample_rows(owned, 1, Fill::Short);
    let short_bytes = encode_rows(owned, short_row)
        .map(|bytes| bytes.len().saturating_sub(2).max(1))
        .unwrap_or(MAX_BLOB_FIELD_BYTES);
    let fitting = (MAX_BLOB_FIELD_BYTES - 2) / short_bytes;
    let literal_limit = keys
        .iter()
        .map(|key| owned_literals(owned.table, key).map_or(usize::MAX, <[&str]>::len))
        .try_fold(1_usize, |product, count| product.checked_mul(count))
        .unwrap_or(usize::MAX);
    vec![
        Vec::new(),
        sample_rows(owned, 1, Fill::Long(long_chars)),
        sample_rows(
            owned,
            owned.max_rows.min(fitting).min(literal_limit),
            Fill::Short,
        ),
    ]
}

/// Rules of Calendar values the column types cannot express: enumerations, the geographic
/// pair, required references, and the owned row codec.
#[derive(Debug, Clone, Copy, Default)]
pub struct CalendarAdapter;

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

/// Every field of the value is set, as the schema's `NOT NULL` row keys require.
fn required(value: &Value, what: &'static str) -> Result<(), AdapterError> {
    value.fields().iter().try_for_each(|field| match field {
        Field::Null => Err(AdapterError(what)),
        _ => Ok(()),
    })
}

/// Both coordinates are set or both are null.
fn geo_pair(latitude: &Field, longitude: &Field) -> Result<(), AdapterError> {
    match (latitude, longitude) {
        (Field::Null, Field::Null) | (Field::Integer(_), Field::Integer(_)) => Ok(()),
        _ => Err(AdapterError("calendar event coordinates")),
    }
}

fn validate_event(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    let fields = value.fields();
    match (group, fields) {
        (event_group::CALENDAR, _) => required(value, "calendar event calendar"),
        (event_group::PLACE, [_, latitude, longitude]) => geo_pair(latitude, longitude),
        (event_group::AVAILABILITY, [transparency, status]) => {
            one_of(
                transparency,
                allowed::TRANSPARENCIES,
                "calendar event transparency",
            )?;
            one_of(status, allowed::EVENT_STATUSES, "calendar event status")
        }
        (event_group::PRIVACY, [visibility]) => one_of(
            visibility,
            allowed::VISIBILITIES,
            "calendar event visibility",
        ),
        (event_group::RSVP, [status]) => optional_one_of(
            status,
            allowed::PARTICIPATION_STATUSES,
            "calendar event RSVP status",
        ),
        (
            event_group::PLACE
            | event_group::AVAILABILITY
            | event_group::PRIVACY
            | event_group::RSVP,
            _,
        ) => Err(AdapterError("calendar event value")),
        _ => Ok(()),
    }
}

fn validate_archive(group: GroupId, value: &Value) -> Result<(), AdapterError> {
    let fields = value.fields();
    match group {
        archive_group::EVENT => match fields.get(3) {
            Some(Field::Null) => Ok(()),
            Some(Field::Text(date)) if date.chars().count() == RECURRENCE_DATE_CHARS => Ok(()),
            _ => Err(AdapterError("calendar archive recurrence date")),
        },
        archive_group::DETAILS => {
            let [
                _,
                _,
                _,
                _,
                transparency,
                status,
                visibility,
                _,
                latitude,
                longitude,
                rsvp,
                _,
            ] = fields
            else {
                return Err(AdapterError("calendar archive details"));
            };
            one_of(
                transparency,
                allowed::TRANSPARENCIES,
                "calendar archive transparency",
            )?;
            one_of(status, allowed::EVENT_STATUSES, "calendar archive status")?;
            one_of(
                visibility,
                allowed::VISIBILITIES,
                "calendar archive visibility",
            )?;
            optional_one_of(
                rsvp,
                allowed::PARTICIPATION_STATUSES,
                "calendar archive RSVP status",
            )?;
            geo_pair(latitude, longitude)
        }
        _ => Ok(()),
    }
}

impl DomainAdapter for CalendarAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn encode_owned(
        &self,
        table: TableId,
        owned: &str,
        rows: Vec<Vec<Field>>,
    ) -> Result<Vec<u8>, AdapterError> {
        encode_rows(owned_table(table, owned)?, rows)
    }

    fn decode_owned(
        &self,
        table: TableId,
        owned: &str,
        bytes: &[u8],
    ) -> Result<Vec<Vec<Field>>, AdapterError> {
        decode_rows(owned_table(table, owned)?, bytes)
    }

    fn owned_samples(&self, table: TableId, owned: &str) -> Vec<Vec<Vec<Field>>> {
        owned_table(table, owned)
            .map(owned_samples)
            .unwrap_or_default()
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
        let fields = value.fields();
        match table {
            CALENDARS_TABLE if group == calendar_group::SOURCE => match fields {
                [source, _, _] => one_of(source, allowed::CALENDAR_SOURCES, "calendar source"),
                _ => Err(AdapterError("calendar source")),
            },
            OBJECTS_TABLE if group == object_group::SOURCE => match fields.get(1) {
                Some(kind) => one_of(
                    kind,
                    allowed::OBJECT_SOURCE_KINDS,
                    "iCalendar object source kind",
                ),
                None => Err(AdapterError("iCalendar object source")),
            },
            OBJECTS_TABLE if group == object_group::CALENDAR => {
                required(value, "iCalendar object calendar")
            }
            COMPONENTS_TABLE if group == component_group::OBJECT => {
                required(value, "iCalendar component object")
            }
            COMPONENTS_TABLE if group == component_group::CALENDAR => {
                required(value, "iCalendar component calendar")
            }
            COMPONENTS_TABLE if group == component_group::COMPONENT => match fields.get(6) {
                Some(status) => one_of(
                    status,
                    allowed::PRESERVATION_STATUSES,
                    "iCalendar component preservation status",
                ),
                None => Err(AdapterError("iCalendar component")),
            },
            EVENTS_TABLE => validate_event(group, value),
            ARCHIVES_TABLE => validate_archive(group, value),
            ARCHIVE_OBJECTS_TABLE if group == archive_object_group::IDENTITY => {
                required(value, "calendar archive import object")
            }
            TASK_EVENT_LINKS_TABLE if group == task_event_link_group::IDENTITY => {
                required(value, "project task event link")
            }
            TASK_EVENT_LINKS_TABLE if group == task_event_link_group::KIND => match fields {
                [kind] => one_of(
                    kind,
                    allowed::TASK_LINK_KINDS,
                    "project task event link kind",
                ),
                _ => Err(AdapterError("project task event link kind")),
            },
            _ => Ok(()),
        }
    }

    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let mut presentation = Presentation::default();
        for (group, value) in values {
            let first = match value.fields().first() {
                Some(Field::Text(text)) => text,
                _ => continue,
            };
            match (table, *group) {
                (CALENDARS_TABLE, calendar_group::NAME)
                | (EVENTS_TABLE, event_group::TITLE)
                | (ARCHIVES_TABLE, archive_group::TIMING) => {
                    presentation.title = first.trim_matches(' ').to_owned();
                }
                (EVENTS_TABLE, event_group::DESCRIPTION) => {
                    presentation.preview = preview_text(first);
                }
                _ => {}
            }
        }
        presentation
    }
}

/// Characters of an event description kept as a recovery preview.
const PREVIEW_CHARS: usize = 160;

/// A description as one line of at most [`PREVIEW_CHARS`] characters, whitespace collapsed.
fn preview_text(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(PREVIEW_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::vault::calendar::{ARCHIVE_MUSIC, EXDATES, OVERRIDE_PROPERTIES};

    fn text(value: &str) -> Field {
        Field::Text(value.to_owned())
    }

    #[test]
    fn keyed_rows_must_ascend_without_repeats() {
        let row = |date: &str| vec![text(date), text("id")];
        assert!(encode_rows(&EXDATES, vec![row("2026-01-01"), row("2026-01-02")]).is_ok());
        assert!(encode_rows(&EXDATES, vec![row("2026-01-02"), row("2026-01-01")]).is_err());
        assert!(encode_rows(&EXDATES, vec![row("2026-01-01"), row("2026-01-01")]).is_err());
    }

    #[test]
    fn composite_keys_order_by_their_first_differing_column() {
        let row = |override_id: &str, key: &str| {
            vec![
                text(override_id),
                text(key),
                text("id"),
                text("value"),
                Field::Integer(0),
            ]
        };
        let ordered = vec![row("a", "z"), row("b", "a"), row("b", "b")];
        let bytes = encode_rows(&OVERRIDE_PROPERTIES, ordered.clone()).unwrap();
        assert_eq!(decode_rows(&OVERRIDE_PROPERTIES, &bytes).unwrap(), ordered);
        assert!(encode_rows(&OVERRIDE_PROPERTIES, vec![row("b", "a"), row("a", "z")]).is_err());
    }

    #[test]
    fn owned_enumerations_and_blank_ids_are_refused() {
        let assignment = |phase: &str, preset: Field| {
            vec![
                text("event-snapshot"),
                text(phase),
                text("inherit"),
                Field::Null,
                text("explicit"),
                Field::Null,
                Field::Integer(1),
                Field::Integer(1),
                text("inherit"),
                preset,
            ]
        };
        assert!(encode_rows(&ARCHIVE_MUSIC, vec![assignment("focus", Field::Null)]).is_ok());
        assert!(encode_rows(&ARCHIVE_MUSIC, vec![assignment("lunch", Field::Null)]).is_err());
        assert!(encode_rows(&ARCHIVE_MUSIC, vec![assignment("focus", text(" "))]).is_err());
    }

    #[test]
    fn samples_fit_one_blob_field_and_encode() {
        for table in DOMAIN.tables {
            for owned in table.groups.iter().flat_map(|group| group.owned_tables()) {
                let samples = owned_samples(owned);
                assert_eq!(samples.len(), 3, "{}", owned.table);
                for rows in samples {
                    let bytes = encode_rows(owned, rows)
                        .unwrap_or_else(|error| panic!("{}: {error}", owned.table));
                    assert!(bytes.len() <= MAX_BLOB_FIELD_BYTES, "{}", owned.table);
                }
            }
        }
    }

    #[test]
    fn coordinates_are_set_together() {
        let adapter = CalendarAdapter;
        let place = |latitude: Field, longitude: Field| {
            Value::new(vec![text(""), latitude, longitude]).unwrap()
        };
        let check = |value| adapter.validate(EVENTS_TABLE, event_group::PLACE, &value);
        assert!(check(place(Field::Null, Field::Null)).is_ok());
        assert!(check(place(Field::Integer(1), Field::Integer(-1))).is_ok());
        assert!(check(place(Field::Integer(1), Field::Null)).is_err());
    }
}

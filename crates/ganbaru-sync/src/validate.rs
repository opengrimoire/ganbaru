//! Manifest validation of group values and changes.
//!
//! Column rules mirror the value guard triggers in [`crate::triggers`]: characters are Unicode
//! scalar values, blank means only U+0020 spaces, and order keys follow the order key grammar.
//! A test compares both forms on the same samples.

use ganbaru_sync_contracts::bounds::{MAX_ROW_KEY_BYTES, MAX_TEXT_FIELD_BYTES};
use ganbaru_sync_contracts::{Change, ChangeAction, Field, OrderKey, RowKey, Value};

use crate::Engine;
use crate::codec::{decode_table, owned_blob};
use crate::manifest::{ColumnType, GroupSpec, MAX_TIMESTAMP_BYTES, MergeKind, TableSpec};

/// Why a value or change breaks the manifest.
pub(crate) type Invalid = &'static str;

fn is_text_without_nul(field: &Field) -> Option<&str> {
    match field {
        Field::Text(text) if !text.contains('\0') => Some(text),
        _ => None,
    }
}

fn is_text_within(field: &Field, max_chars: usize) -> bool {
    is_text_without_nul(field)
        .is_some_and(|text| text.len() <= MAX_TEXT_FIELD_BYTES && text.chars().count() <= max_chars)
}

fn is_timestamp(field: &Field) -> bool {
    is_text_without_nul(field)
        .is_some_and(|text| !text.trim_matches(' ').is_empty() && text.len() <= MAX_TIMESTAMP_BYTES)
}

/// Whether a field satisfies a column rule.
pub(crate) fn column_value(ty: ColumnType, field: &Field) -> bool {
    match ty {
        ColumnType::Text { max_chars } => is_text_within(field, max_chars),
        ColumnType::OptionalText { max_chars } => {
            *field == Field::Null || is_text_within(field, max_chars)
        }
        ColumnType::TrimmedText {
            min_chars,
            max_chars,
        } => is_text_without_nul(field).is_some_and(|text| {
            let chars = text.trim_matches(' ').chars().count();
            text.len() <= MAX_TEXT_FIELD_BYTES && (min_chars..=max_chars).contains(&chars)
        }),
        ColumnType::Integer { min, max } => {
            matches!(field, Field::Integer(value) if (min..=max).contains(value))
        }
        ColumnType::OptionalInteger { min, max } => match field {
            Field::Null => true,
            Field::Integer(value) => (min..=max).contains(value),
            _ => false,
        },
        ColumnType::Flag => matches!(field, Field::Integer(0 | 1)),
        ColumnType::Timestamp => is_timestamp(field),
        ColumnType::OptionalTimestamp => *field == Field::Null || is_timestamp(field),
        ColumnType::OrderKey => {
            is_text_without_nul(field).is_some_and(|text| OrderKey::parse(text).is_ok())
        }
        ColumnType::OptionalRowKey => match field {
            Field::Null => true,
            Field::Text(text) => {
                text.len() <= MAX_ROW_KEY_BYTES && RowKey::new(text.as_str()).is_ok()
            }
            _ => false,
        },
    }
}

/// Checks a group value against its storage, column rules, and the table's adapter.
pub(crate) fn group_value(
    engine: &Engine,
    table: &TableSpec,
    group: &GroupSpec,
    value: &Value,
) -> Result<(), Invalid> {
    if table.is_exists_group(group.id) {
        return match value.fields() {
            [Field::Integer(0 | 1)] => Ok(()),
            _ => Err("existence value"),
        };
    }
    if value.fields().len() != group.field_count() {
        return Err("group field count");
    }
    let columns = group.columns();
    for (column, field) in columns.iter().zip(value.fields()) {
        if !column_value(column.ty, field) {
            return Err("column value");
        }
    }
    for (index, owned) in group.owned_tables().iter().enumerate() {
        decode_table(engine, table, owned, owned_blob(group, value, index)?)?;
    }
    if let Some(adapter) = engine.adapter(table) {
        adapter
            .validate(table.id, group.id, value)
            .map_err(|error| error.0)?;
    }
    Ok(())
}

/// Checks one change against the manifest.
pub(crate) fn change(engine: &Engine, change: &Change) -> Result<(), Invalid> {
    let table = engine.manifest().table(change.table).ok_or("table")?;
    let mask = change.mask();
    let full = table.full_mask();
    match change.action {
        ChangeAction::Create => {
            if mask != full {
                return Err("create groups");
            }
        }
        ChangeAction::Write => {
            if mask.union(full) != full {
                return Err("write groups");
            }
            // A derived-key row added again rewrites every group, its key-only immutable groups
            // with the values they already hold.
            let immutable = table
                .groups
                .iter()
                .any(|group| group.kind == MergeKind::Immutable && mask.contains(group.id));
            let readd = table.derived_key.is_some() && mask == full;
            if immutable && !readd {
                return Err("immutable group write");
            }
        }
        ChangeAction::Tombstone => {
            if table.derived_key.is_some() {
                return Err("derived-key tombstone");
            }
            if let Some(target) = &change.replaced_by {
                if !table.redirects || target >= &change.row {
                    return Err("tombstone redirect");
                }
            }
            if let Some(cause) = &change.cause {
                if engine.manifest().table(cause.table).is_none() {
                    return Err("tombstone cause");
                }
            }
        }
    }
    for value in &change.groups {
        let group = table.group(value.group).ok_or("group")?;
        group_value(engine, table, group, &value.value)?;
    }
    Ok(())
}

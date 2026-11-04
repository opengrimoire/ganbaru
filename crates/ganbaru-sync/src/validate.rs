//! Manifest validation of group values and changes.
//!
//! Column rules mirror the value guard triggers in [`crate::triggers`]: characters are Unicode
//! scalar values, blank means only U+0020 spaces, and order keys follow the order key grammar.
//! A test compares both forms on the same samples.

use ganbaru_sync_contracts::bounds::{MAX_ROW_KEY_BYTES, MAX_TEXT_FIELD_BYTES};
use ganbaru_sync_contracts::{
    Change, ChangeAction, Field, OrderKey, RowKey, Value, derived_key, device_owner_tag,
};

use crate::Engine;
use crate::codec::{decode_table, owned_blob};
use crate::manifest::{
    ColumnType, Deletion, DerivedKey, GroupSpec, MAX_TIMESTAMP_BYTES, MergeKind, RowOwner,
    TableSpec,
};

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
        ColumnType::OptionalRowKey => *field == Field::Null || is_row_key_text(field),
        ColumnType::RowKey => is_row_key_text(field),
    }
}

/// Whether a field is text that can key a row.
fn is_row_key_text(field: &Field) -> bool {
    matches!(
        field,
        Field::Text(text) if text.len() <= MAX_ROW_KEY_BYTES && RowKey::new(text.as_str()).is_ok()
    )
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
    if !table.key_rule.admits(change.row.as_str()) {
        return Err("row key rule");
    }
    if let Some(adapter) = engine.adapter(table) {
        adapter
            .validate_key(table.id, change.row.as_str())
            .map_err(|error| error.0)?;
    }
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
            if table.deletion == Deletion::Never {
                return Err("tombstone of a never-deleted table");
            }
            if let Some(target) = &change.replaced_by {
                if !table.redirects || target >= &change.row {
                    return Err("tombstone redirect");
                }
            }
            if let Some(cause) = &change.cause {
                // Every honest cause is reached along references, and derivation scoped to a
                // component relies on a recovery read never crossing one.
                if !engine.same_component(table.id, cause.table) {
                    return Err("tombstone cause");
                }
            }
        }
    }
    for value in &change.groups {
        let group = table.group(value.group).ok_or("group")?;
        group_value(engine, table, group, &value.value)?;
    }
    let writes_every_group = match change.action {
        ChangeAction::Create => true,
        ChangeAction::Write => mask == full,
        ChangeAction::Tombstone => false,
    };
    if let Some(derived) = table.derived_key.filter(|_| writes_every_group) {
        derived_row_key(table, derived, change)?;
    }
    if writes_every_group && let Some(adapter) = engine.adapter(table) {
        adapter
            .validate_row(table.id, &change.row, &change.groups)
            .map_err(|error| error.0)?;
    }
    Ok(())
}

/// Refuses a change to a device-owned row from any other device.
pub(crate) const OWNER_MISMATCH: Invalid = "row owner mismatch";

/// Checks that `writer_device` owns the row `change` writes, and that a create names that device
/// in its device column. A write of existence groups alone is left to the group merge.
pub(crate) fn writer_change(
    table: &TableSpec,
    change: &Change,
    writer_device: &str,
) -> Result<(), Invalid> {
    let Some(owner) = table.owner else {
        return Ok(());
    };
    let writes_existence_only = change.action == ChangeAction::Write
        && !change.groups.is_empty()
        && change
            .groups
            .iter()
            .all(|value| table.is_exists_group(value.group));
    if writes_existence_only {
        return Ok(());
    }
    if !device_owns_row(table, change.row.as_str(), writer_device) {
        return Err(OWNER_MISMATCH);
    }
    if change.action == ChangeAction::Create
        && !creates_for_device(table, change, owner.device_column(), writer_device)
    {
        return Err(OWNER_MISMATCH);
    }
    Ok(())
}

/// Whether the create names `device` in `column`, which a key column does by the key itself.
fn creates_for_device(table: &TableSpec, change: &Change, column: &str, device: &str) -> bool {
    let located = table.groups.iter().find_map(|group| {
        let index = group
            .columns()
            .iter()
            .position(|spec| spec.name == column)?;
        Some((group.id, index))
    });
    let Some((group, index)) = located else {
        return true;
    };
    change
        .groups
        .iter()
        .find(|value| value.group == group)
        .and_then(|value| value.value.fields().get(index))
        .is_some_and(|field| matches!(field, Field::Text(text) if text == device))
}

/// True when `table` has no owner, or when `row_key` names `device`.
pub(crate) fn device_owns_row(table: &TableSpec, row_key: &str, device: &str) -> bool {
    match table.owner {
        None => true,
        Some(RowOwner::KeyIsDevice { .. }) => row_key == device,
        Some(RowOwner::KeyTag { prefix, .. }) => row_key
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_prefix(device_owner_tag(device).as_str()))
            .is_some_and(|rest| rest.starts_with(':')),
    }
}

/// Checks that a change writing every group of a derived-key row names the key its key inputs
/// derive, so no writer can file content under a key that content does not produce. Text
/// inputs are parts as they are, integers in decimal, and any other field never matches.
fn derived_row_key(table: &TableSpec, derived: DerivedKey, change: &Change) -> Result<(), Invalid> {
    let Some(prefix) = derived.prefix else {
        return Ok(());
    };
    let mut parts = Vec::with_capacity(derived.key_inputs.len());
    for input in derived.key_inputs {
        let field = table.groups.iter().find_map(|group| {
            let index = group
                .columns()
                .iter()
                .position(|column| column.name == *input)?;
            let value = change.groups.iter().find(|value| value.group == group.id)?;
            value.value.fields().get(index)
        });
        parts.push(match field {
            Some(Field::Text(text)) => text.clone(),
            Some(Field::Integer(integer)) => integer.to_string(),
            _ => return Err(DERIVED_KEY_MISMATCH),
        });
    }
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    match derived_key(prefix, &parts) {
        Ok(key) if key == change.row => Ok(()),
        _ => Err(DERIVED_KEY_MISMATCH),
    }
}

/// Why a create or re-add of a derived-key row is refused when its key is not the one its key
/// inputs derive.
pub(crate) const DERIVED_KEY_MISMATCH: Invalid = "derived key mismatch";

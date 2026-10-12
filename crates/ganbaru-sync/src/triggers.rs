//! Capture and guard trigger SQL rendered from the manifest.
//!
//! Migrations install these triggers as persistent schema objects. The conformance test compares
//! `sqlite_schema` with [`render`], so a manifest change without a matching migration fails.
//!
//! - Capture triggers upsert one `sync_capture` row per changed domain row: inserts set
//!   `created` and the full mask, updates OR one bit per changed group, and deletes set
//!   `deleted`. Owned child rows mark their parent's group.
//! - A reference group change to null is not captured when the old target no longer exists:
//!   that change is the foreign key action of a deletion, which the target's tombstone already
//!   expresses.
//! - Value guards enforce the manifest value rules on every write, so every locally valid row
//!   seals into a valid operation.
//! - Row keys never change, and immutable groups change only while the engine applies.
//! - A published key that is no longer present cannot be inserted again locally.
//!
//! Capture triggers and the immutable and re-create guards are disabled while
//! `sync_apply_state.applying` is 1; value guards always run.

use std::fmt::Write as _;

use ganbaru_sync_contracts::bounds::{
    MAX_ORDER_KEY_BYTES, MAX_ROW_KEY_BYTES, MAX_TEXT_FIELD_BYTES,
};

use crate::manifest::{
    ColumnSpec, ColumnType, GroupStorage, MAX_TIMESTAMP_BYTES, Manifest, MergeKind, TableSpec,
};

/// Trigger condition true outside engine apply transactions.
const NOT_APPLYING: &str = "(SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0";

/// Capture time in Unix milliseconds.
const NOW_MS: &str = "CAST(unixepoch('subsec') * 1000 AS INTEGER)";

const CAPTURE_INSERT: &str =
    "INSERT INTO sync_capture (table_id, row_key, mask, created, deleted, captured_at_ms)";

const MERGE_MASK: &str =
    "ON CONFLICT (table_id, row_key) DO UPDATE SET mask = mask | excluded.mask";

/// One rendered trigger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriggerSql {
    /// Trigger name.
    pub name: String,
    /// `CREATE TRIGGER` statement as stored in `sqlite_schema`, without a trailing semicolon.
    pub sql: String,
}

/// Every capture and guard trigger the manifest requires, in installation order.
pub fn render(manifest: &Manifest) -> Vec<TriggerSql> {
    let mut triggers = Vec::new();
    for table in manifest.tables {
        triggers.push(value_guard(table, Event::Insert));
        triggers.push(value_guard(table, Event::Update));
        triggers.push(immutable_guard(table));
        triggers.push(recreate_guard(table));
        triggers.push(insert_capture(table));
        triggers.push(update_capture(manifest, table));
        triggers.push(delete_capture(table));
        for (group, owned) in table.owned_children() {
            let bit = mask_literal(group.mask().0);
            for event in [Event::Insert, Event::Update, Event::Delete] {
                triggers.push(owned_capture(
                    table,
                    owned.table,
                    owned.parent_column,
                    &bit,
                    event,
                ));
            }
        }
    }
    triggers
}

/// Collapses whitespace so stored and rendered trigger SQL compare equal.
pub fn normalize(sql: &str) -> String {
    sql.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Event {
    Insert,
    Update,
    Delete,
}

impl Event {
    const fn keyword(self) -> &'static str {
        match self {
            Self::Insert => "INSERT",
            Self::Update => "UPDATE",
            Self::Delete => "DELETE",
        }
    }

    const fn suffix(self) -> &'static str {
        match self {
            Self::Insert => "insert",
            Self::Update => "update",
            Self::Delete => "delete",
        }
    }
}

fn mask_literal(mask: u64) -> String {
    // SQLite integers are signed; bit 63 stores as a negative value.
    (mask as i64).to_string()
}

fn header(name: &str, timing: &str, event: &str, table: &str) -> String {
    format!("CREATE TRIGGER {name}\n{timing} {event} ON {table}\n")
}

fn abort_body(message: &str) -> String {
    format!("BEGIN\n    SELECT RAISE(ABORT, '{message}');\nEND")
}

/// Text without NUL, which SQLite text functions would truncate.
fn text_rule(expr: &str) -> String {
    format!("typeof({expr}) = 'text' AND instr(CAST({expr} AS BLOB), x'00') = 0")
}

fn byte_length(expr: &str) -> String {
    format!("length(CAST({expr} AS BLOB))")
}

pub(crate) fn row_key_rule(expr: &str) -> String {
    format!(
        "{} AND trim({expr}) <> '' AND {} <= {MAX_ROW_KEY_BYTES}",
        text_rule(expr),
        byte_length(expr)
    )
}

fn timestamp_rule(expr: &str) -> String {
    format!(
        "{} AND trim({expr}) <> '' AND {} <= {MAX_TIMESTAMP_BYTES}",
        text_rule(expr),
        byte_length(expr)
    )
}

/// The order key grammar: base 62 digits, a head giving the integer length, no fraction ending
/// in `0`, and not the smallest integer alone.
fn order_key_rule(expr: &str) -> String {
    let integer_length = format!(
        "(CASE WHEN unicode({expr}) BETWEEN 97 AND 122 THEN unicode({expr}) - 95 \
         WHEN unicode({expr}) BETWEEN 65 AND 90 THEN 92 - unicode({expr}) \
         ELSE {} END)",
        MAX_ORDER_KEY_BYTES + 1
    );
    let smallest = format!("A{}", "0".repeat(26));
    format!(
        "{} AND {expr} NOT GLOB '*[^0-9A-Za-z]*' AND length({expr}) <= {MAX_ORDER_KEY_BYTES} \
         AND {integer_length} <= length({expr}) \
         AND ({integer_length} = length({expr}) OR substr({expr}, -1) <> '0') \
         AND {expr} <> '{smallest}'",
        text_rule(expr)
    )
}

/// SQL that is 1 exactly when `expr` satisfies the column type's value rule.
pub(crate) fn value_rule(expr: &str, ty: ColumnType) -> String {
    match ty {
        ColumnType::Text { max_chars } => format!(
            "{} AND length({expr}) <= {max_chars} AND {} <= {MAX_TEXT_FIELD_BYTES}",
            text_rule(expr),
            byte_length(expr)
        ),
        ColumnType::TrimmedText {
            min_chars,
            max_chars,
        } => format!(
            "{} AND length(trim({expr})) BETWEEN {min_chars} AND {max_chars} AND {} <= {MAX_TEXT_FIELD_BYTES}",
            text_rule(expr),
            byte_length(expr)
        ),
        ColumnType::Integer { min, max } => {
            format!("typeof({expr}) = 'integer' AND {expr} BETWEEN {min} AND {max}")
        }
        ColumnType::Flag => format!("typeof({expr}) = 'integer' AND {expr} IN (0, 1)"),
        ColumnType::Timestamp => timestamp_rule(expr),
        ColumnType::OptionalTimestamp => {
            format!("{expr} IS NULL OR ({})", timestamp_rule(expr))
        }
        ColumnType::OrderKey => order_key_rule(expr),
        ColumnType::OptionalRowKey => format!("{expr} IS NULL OR ({})", row_key_rule(expr)),
    }
}

fn group_columns(table: &TableSpec) -> impl Iterator<Item = &'static ColumnSpec> {
    table.groups.iter().flat_map(|group| group.columns())
}

/// Rejects writes whose key or group values break the manifest value rules.
fn value_guard(table: &TableSpec, event: Event) -> TriggerSql {
    let name = format!("sync_values_{}_{}", table.name, event.suffix());
    let mut rules = Vec::new();
    if event == Event::Insert {
        rules.push(row_key_rule(&format!("NEW.{}", table.key_column)));
    }
    rules.extend(
        group_columns(table).map(|column| value_rule(&format!("NEW.{}", column.name), column.ty)),
    );
    let keyword = match event {
        Event::Update => {
            let columns = group_columns(table)
                .map(|column| column.name)
                .collect::<Vec<_>>()
                .join(", ");
            format!("UPDATE OF {columns}")
        }
        Event::Insert | Event::Delete => event.keyword().to_string(),
    };
    let condition = rules
        .iter()
        .map(|rule| format!("({rule})"))
        .collect::<Vec<_>>()
        .join("\n    AND ");
    let mut sql = header(&name, "BEFORE", &keyword, table.name);
    let _ = write!(
        sql,
        "WHEN ({condition}) IS NOT 1\n{}",
        abort_body("Replicated row values are invalid")
    );
    TriggerSql { name, sql }
}

fn column_changed(column: &ColumnSpec) -> String {
    let collate = if column.ty.is_text() {
        " COLLATE BINARY"
    } else {
        ""
    };
    format!("OLD.{name} IS NOT NEW.{name}{collate}", name = column.name)
}

fn immutable_guard(table: &TableSpec) -> TriggerSql {
    let name = format!("sync_immutable_{}", table.name);
    let key = ColumnSpec {
        name: table.key_column,
        ty: ColumnType::OptionalRowKey,
    };
    let immutable = table.immutable_columns().collect::<Vec<_>>();
    let columns = std::iter::once(table.key_column)
        .chain(immutable.iter().map(|column| column.name))
        .collect::<Vec<_>>()
        .join(", ");
    let mut condition = column_changed(&key);
    if !immutable.is_empty() {
        let changed = immutable
            .iter()
            .copied()
            .map(column_changed)
            .collect::<Vec<_>>()
            .join(" OR ");
        let _ = write!(condition, "\n    OR (({changed}) AND {NOT_APPLYING})");
    }
    let mut sql = header(&name, "BEFORE", &format!("UPDATE OF {columns}"), table.name);
    let _ = write!(
        sql,
        "WHEN {condition}\n{}",
        abort_body("Replicated row keys and creation times cannot change")
    );
    TriggerSql { name, sql }
}

fn recreate_guard(table: &TableSpec) -> TriggerSql {
    let name = format!("sync_recreate_{}", table.name);
    let mut sql = header(&name, "BEFORE", "INSERT", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}\n    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = {id} AND row_key = NEW.{key})\n    AND NOT EXISTS (SELECT 1 FROM {table} WHERE {key} = NEW.{key})\n{}",
        abort_body("A published row key cannot be created again"),
        id = table.id.0,
        key = table.key_column,
        table = table.name,
    );
    TriggerSql { name, sql }
}

fn insert_capture(table: &TableSpec) -> TriggerSql {
    let name = format!("sync_capture_{}_insert", table.name);
    let mut sql = header(&name, "AFTER", "INSERT", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}\nBEGIN\n    {CAPTURE_INSERT}\n    VALUES ({id}, NEW.{key}, {mask}, 1, 0, {NOW_MS})\n    ON CONFLICT (table_id, row_key) DO UPDATE SET\n        mask = mask | excluded.mask, created = 1, deleted = 0;\nEND",
        id = table.id.0,
        key = table.key_column,
        mask = mask_literal(table.full_mask().0),
    );
    TriggerSql { name, sql }
}

fn group_term(
    manifest: &Manifest,
    table: &TableSpec,
    columns: &[ColumnSpec],
    kind: MergeKind,
    bit: &str,
) -> String {
    let changed = columns
        .iter()
        .map(column_changed)
        .collect::<Vec<_>>()
        .join(" OR ");
    match kind {
        MergeKind::Reference { table: target } => {
            let target = manifest
                .table(target)
                .unwrap_or_else(|| panic!("{} references an unknown table", table.name));
            let column = columns[0].name;
            format!(
                "(CASE WHEN {changed}\n                AND NOT (NEW.{column} IS NULL AND NOT EXISTS (SELECT 1 FROM {target_table} WHERE {target_key} = OLD.{column}))\n                THEN {bit} ELSE 0 END)",
                target_table = target.name,
                target_key = target.key_column,
            )
        }
        _ => format!("(CASE WHEN {changed} THEN {bit} ELSE 0 END)"),
    }
}

fn update_capture(manifest: &Manifest, table: &TableSpec) -> TriggerSql {
    let name = format!("sync_capture_{}_update", table.name);
    let terms = table
        .groups
        .iter()
        .filter(|group| group.kind != MergeKind::Immutable)
        .filter_map(|group| match group.storage {
            GroupStorage::Columns(columns) => Some(group_term(
                manifest,
                table,
                columns,
                group.kind,
                &mask_literal(group.mask().0),
            )),
            GroupStorage::Owned(_) => None,
        })
        .collect::<Vec<_>>()
        .join("\n            | ");
    let mut sql = header(&name, "AFTER", "UPDATE", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}\nBEGIN\n    {CAPTURE_INSERT}\n    SELECT {id}, NEW.{key}, changed.mask, 0, 0, {NOW_MS}\n    FROM (\n        SELECT\n            {terms}\n            AS mask\n    ) AS changed\n    WHERE changed.mask <> 0\n    {MERGE_MASK};\nEND",
        id = table.id.0,
        key = table.key_column,
    );
    TriggerSql { name, sql }
}

fn delete_capture(table: &TableSpec) -> TriggerSql {
    let name = format!("sync_capture_{}_delete", table.name);
    let mut sql = header(&name, "AFTER", "DELETE", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}\nBEGIN\n    {CAPTURE_INSERT}\n    VALUES ({id}, OLD.{key}, 0, 0, 1, {NOW_MS})\n    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;\nEND",
        id = table.id.0,
        key = table.key_column,
    );
    TriggerSql { name, sql }
}

fn owned_capture(
    parent: &TableSpec,
    child: &str,
    parent_column: &str,
    bit: &str,
    event: Event,
) -> TriggerSql {
    let name = format!("sync_capture_{child}_{}", event.suffix());
    let id = parent.id.0;
    let body = match event {
        Event::Insert | Event::Delete => {
            let row = if event == Event::Insert { "NEW" } else { "OLD" };
            format!(
                "    {CAPTURE_INSERT}\n    VALUES ({id}, {row}.{parent_column}, {bit}, 0, 0, {NOW_MS})\n    {MERGE_MASK};"
            )
        }
        Event::Update => format!(
            "    {CAPTURE_INSERT}\n    SELECT {id}, parent.key, {bit}, 0, 0, {NOW_MS}\n    FROM (SELECT NEW.{parent_column} AS key UNION SELECT OLD.{parent_column}) AS parent\n    WHERE true\n    {MERGE_MASK};"
        ),
    };
    let mut sql = header(&name, "AFTER", event.keyword(), child);
    let _ = write!(sql, "WHEN {NOT_APPLYING}\nBEGIN\n{body}\nEND");
    TriggerSql { name, sql }
}

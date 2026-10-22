//! Capture and guard trigger SQL rendered from the manifest.
//!
//! Migrations install these triggers as persistent schema objects. The conformance test compares
//! `sqlite_schema` with [`render`], so a manifest change without a matching migration fails.
//!
//! - Capture triggers upsert one `sync_capture` row per changed domain row: inserts set
//!   `created` and the full mask, updates OR one bit per changed group, and deletes set
//!   `deleted`. Owned child rows mark their parent's group.
//! - A reference column change to null is not captured when the old target no longer exists:
//!   that change is the foreign key action of a deletion, which the target's tombstone already
//!   expresses. Likewise, an owned row deleted because its referenced row or its parent owned
//!   row was deleted is not captured.
//! - Value guards enforce the manifest value rules on every write, so every locally valid row
//!   seals into a valid operation. Generic codec owned tables have guards of their own; a custom
//!   codec's adapter refuses rows it cannot encode when the group seals.
//! - Row keys never change, and immutable groups change only while the engine applies. A
//!   `sync_key` that starts with the `pending:` placeholder prefix may change once to a valid
//!   key, which captures the row as inserted under its final key.
//! - A published key that is no longer present cannot be inserted again locally, except on
//!   derived-key tables, where the insert adds the row again, under a `pending:` key, and as
//!   the Undo of a held delete, which turns the held capture into an ordinary write.
//! - On a table that publishes conditionally, inserts and updates are captured only while the
//!   row is published. The update that publishes a row captures it as created with every group,
//!   together with every child row published with it, and a publication guard refuses an
//!   update that turns a published row back into a draft. Deleting a row that was never
//!   published or captured records nothing.
//!
//! Capture triggers and the immutable, re-create, and publication guards are disabled while
//! `sync_apply_state.applying` is 1; value guards always run.

use std::fmt::Write as _;

use ganbaru_sync_contracts::PENDING_KEY_PREFIX;
use ganbaru_sync_contracts::bounds::{
    MAX_ORDER_KEY_BYTES, MAX_ROW_KEY_BYTES, MAX_TEXT_FIELD_BYTES,
};

use crate::manifest::{
    ColumnSpec, ColumnType, GroupSpec, MAX_TIMESTAMP_BYTES, Manifest, MergeKind, OwnedCodec,
    OwnedOrder, OwnedTable, TableSpec,
};
use crate::publication;

/// Trigger condition true outside engine apply transactions.
pub(crate) const NOT_APPLYING: &str =
    "(SELECT applying FROM sync_apply_state WHERE singleton = 1) = 0";

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
        if table.derived_key.is_none() {
            triggers.push(recreate_guard(manifest, table));
        }
        triggers.extend(revert_guard(table));
        triggers.push(insert_capture(manifest, table));
        triggers.push(update_capture(manifest, table));
        triggers.push(delete_capture(manifest, table));
        let published = publication::condition_in(manifest, table, OWNER_ALIAS);
        for group in table.groups {
            let bit = mask_literal(group.mask().0);
            for owned in group.owned_tables() {
                let children = std::iter::once((owned, None))
                    .chain(owned.nested.iter().map(|nested| (nested, Some(owned))));
                for (child, parent_row) in children {
                    if child.codec == OwnedCodec::Generic {
                        triggers.push(owned_value_guard(child, Event::Insert));
                        triggers.push(owned_value_guard(child, Event::Update));
                    }
                    let capture = OwnedCapture {
                        parent: table,
                        owned: child,
                        parent_row,
                        bit: &bit,
                        published: published.as_deref(),
                    };
                    for event in [Event::Insert, Event::Update, Event::Delete] {
                        triggers.push(owned_capture(manifest, &capture, event));
                    }
                }
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

fn text_within_rule(expr: &str, max_chars: usize) -> String {
    format!(
        "{} AND length({expr}) <= {max_chars} AND {} <= {MAX_TEXT_FIELD_BYTES}",
        text_rule(expr),
        byte_length(expr)
    )
}

/// SQL that is 1 exactly when `expr` satisfies the column type's value rule.
pub(crate) fn value_rule(expr: &str, ty: ColumnType) -> String {
    match ty {
        ColumnType::Text { max_chars } => text_within_rule(expr, max_chars),
        ColumnType::OptionalText { max_chars } => {
            format!("{expr} IS NULL OR ({})", text_within_rule(expr, max_chars))
        }
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

fn group_columns(table: &TableSpec) -> impl Iterator<Item = &'static ColumnSpec> + Clone {
    table.groups.iter().flat_map(|group| group.columns())
}

/// Rejects writes whose key or group values break the manifest value rules.
fn value_guard(table: &TableSpec, event: Event) -> TriggerSql {
    let key = (event == Event::Insert).then(|| row_key_rule(&format!("NEW.{}", table.key_column)));
    column_guard(table.name, key, group_columns(table), event)
}

/// Rejects writes whose data columns of a generic codec owned table break their value rules.
fn owned_value_guard(owned: &OwnedTable, event: Event) -> TriggerSql {
    column_guard(owned.table, None, owned.columns.iter(), event)
}

fn column_guard<'a>(
    table: &str,
    key: Option<String>,
    columns: impl Iterator<Item = &'a ColumnSpec> + Clone,
    event: Event,
) -> TriggerSql {
    let name = format!("sync_values_{table}_{}", event.suffix());
    let mut rules = Vec::from_iter(key);
    rules.extend(
        columns
            .clone()
            .map(|column| value_rule(&format!("NEW.{}", column.name), column.ty)),
    );
    let keyword = match event {
        Event::Update => {
            let columns = columns
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
    let mut sql = header(&name, "BEFORE", &keyword, table);
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
    if table.has_composite_key() {
        condition = format!(
            "({condition}\n        AND (OLD.{name} NOT GLOB {pending} OR ({rule}) IS NOT 1 OR NEW.{name} GLOB {pending}))",
            name = table.key_column,
            pending = pending_glob(),
            rule = row_key_rule(&format!("NEW.{}", table.key_column)),
        );
    }
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

/// GLOB pattern of keys holding the `pending:` placeholder prefix.
fn pending_glob() -> String {
    format!("'{PENDING_KEY_PREFIX}*'")
}

fn recreate_guard(manifest: &Manifest, table: &TableSpec) -> TriggerSql {
    let name = format!("sync_recreate_{}", table.name);
    let pending = if table.has_composite_key() {
        format!(
            "\n    AND NEW.{} NOT GLOB {}",
            table.key_column,
            pending_glob()
        )
    } else {
        String::new()
    };
    // Undo of a held delete inserts the row again.
    let held = if manifest.may_hold_deletes(table.id) {
        format!(
            "\n    AND NOT EXISTS (SELECT 1 FROM sync_capture WHERE table_id = {id} AND row_key = NEW.{key} AND held_until_ms IS NOT NULL)",
            id = table.id.0,
            key = table.key_column,
        )
    } else {
        String::new()
    };
    let mut sql = header(&name, "BEFORE", "INSERT", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}{pending}{held}\n    AND EXISTS (SELECT 1 FROM sync_rows WHERE table_id = {id} AND row_key = NEW.{key})\n    AND NOT EXISTS (SELECT 1 FROM {table} WHERE {key} = NEW.{key})\n{}",
        abort_body("A published row key cannot be created again"),
        id = table.id.0,
        key = table.key_column,
        table = table.name,
    );
    TriggerSql { name, sql }
}

/// `AND` and the publication condition of the trigger row `row`, or nothing when every row of
/// the table publishes.
fn published_when(manifest: &Manifest, table: &TableSpec, row: &str) -> String {
    publication::condition_over(manifest, table, row)
        .map(|condition| format!("\n    AND {condition}"))
        .unwrap_or_default()
}

/// Captures an inserted row. Inserting a row whose delete is held reinserts it: the held
/// capture becomes an ordinary write instead of a create.
fn insert_capture(manifest: &Manifest, table: &TableSpec) -> TriggerSql {
    let name = format!("sync_capture_{}_insert", table.name);
    let reinsert = if manifest.may_hold_deletes(table.id) {
        "created = CASE WHEN held_until_ms IS NULL THEN 1 ELSE created END, deleted = 0,\n        held_until_ms = NULL"
    } else {
        "created = 1, deleted = 0"
    };
    let mut sql = header(&name, "AFTER", "INSERT", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}{published}\nBEGIN\n    {CAPTURE_INSERT}\n    VALUES ({id}, NEW.{key}, {mask}, 1, 0, {NOW_MS})\n    ON CONFLICT (table_id, row_key) DO UPDATE SET\n        mask = mask | excluded.mask, {reinsert};\nEND",
        published = published_when(manifest, table, "NEW"),
        id = table.id.0,
        key = table.key_column,
        mask = mask_literal(table.full_mask().0),
    );
    TriggerSql { name, sql }
}

/// Statements of an update that publishes a predicate table row: the row captures as created
/// with every group, and so does every child row published with it.
fn publish_statements(manifest: &Manifest, table: &TableSpec) -> String {
    let Some(was_published) = publication::own_over(table, "OLD") else {
        return String::new();
    };
    let reinsert = "ON CONFLICT (table_id, row_key) DO UPDATE SET\n        mask = mask | excluded.mask, created = 1, deleted = 0;";
    let mut statements = format!(
        "\n    {CAPTURE_INSERT}\n    SELECT {id}, NEW.{key}, {mask}, 1, 0, {NOW_MS}\n    WHERE NOT {was_published}\n    {reinsert}",
        id = table.id.0,
        key = table.key_column,
        mask = mask_literal(table.full_mask().0),
    );
    for child in manifest.tables {
        let Some(with) = child.published_with.filter(|with| with.parent == table.id) else {
            continue;
        };
        let _ = write!(
            statements,
            "\n    {CAPTURE_INSERT}\n    SELECT {id}, child.{key}, {mask}, 1, 0, {NOW_MS}\n    FROM {name} AS child\n    WHERE child.{column} = NEW.{parent_key} AND NOT {was_published}\n    {reinsert}",
            id = child.id.0,
            key = child.key_column,
            mask = mask_literal(child.full_mask().0),
            name = child.name,
            column = with.column,
            parent_key = table.key_column,
        );
    }
    statements
}

/// The mask bit of a column group when any of its columns changed. A reference column that
/// became null while its old target no longer exists is not a change.
fn group_term(manifest: &Manifest, table: &TableSpec, group: &GroupSpec, bit: &str) -> String {
    let columns = group.columns();
    let references = group.reference_columns().collect::<Vec<_>>();
    let changes = columns
        .iter()
        .enumerate()
        .map(|(field, column)| {
            let changed = column_changed(column);
            let Some((_, reference)) = references.iter().find(|(index, _)| *index == field)
            else {
                return changed;
            };
            let target = manifest
                .table(reference.table)
                .unwrap_or_else(|| panic!("{} references an unknown table", table.name));
            let change = format!(
                "{changed}\n                AND NOT (NEW.{name} IS NULL AND NOT EXISTS (SELECT 1 FROM {target_table} WHERE {target_key} = OLD.{name}))",
                name = column.name,
                target_table = target.name,
                target_key = target.key_column,
            );
            if columns.len() == 1 {
                change
            } else {
                format!("({change})")
            }
        })
        .collect::<Vec<_>>()
        .join(" OR ");
    format!("(CASE WHEN {changes}\n                THEN {bit} ELSE 0 END)")
}

fn update_capture(manifest: &Manifest, table: &TableSpec) -> TriggerSql {
    let name = format!("sync_capture_{}_update", table.name);
    let terms = table
        .groups
        .iter()
        .filter(|group| group.kind != MergeKind::Immutable)
        // The existence group has no column, and seals write it from the capture. Owned
        // tables capture their own changes.
        .filter(|group| !group.columns().is_empty())
        .map(|group| group_term(manifest, table, group, &mask_literal(group.mask().0)))
        .collect::<Vec<_>>()
        .join("\n            | ");
    let terms = if terms.is_empty() {
        "0".to_string()
    } else {
        terms
    };
    // A `pending:` key replaced by its final key captures the row as inserted under that key.
    let rekey = if table.has_composite_key() {
        format!(
            "\n    {CAPTURE_INSERT}\n    SELECT {id}, NEW.{key}, {mask}, 1, 0, {NOW_MS}\n    WHERE OLD.{key} IS NOT NEW.{key} COLLATE BINARY\n    ON CONFLICT (table_id, row_key) DO UPDATE SET\n        mask = mask | excluded.mask, created = 1, deleted = 0;",
            id = table.id.0,
            key = table.key_column,
            mask = mask_literal(table.full_mask().0),
        )
    } else {
        String::new()
    };
    let mut sql = header(&name, "AFTER", "UPDATE", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}{published}\nBEGIN\n    {CAPTURE_INSERT}\n    SELECT {id}, NEW.{key}, changed.mask, 0, 0, {NOW_MS}\n    FROM (\n        SELECT\n            {terms}\n            AS mask\n    ) AS changed\n    WHERE changed.mask <> 0\n    {MERGE_MASK};{publish}{rekey}\nEND",
        published = published_when(manifest, table, "NEW"),
        publish = publish_statements(manifest, table),
        id = table.id.0,
        key = table.key_column,
    );
    TriggerSql { name, sql }
}

/// Captures a deleted row. On a table that publishes conditionally, only a row that was
/// published or captured is, so deleting a draft records nothing.
fn delete_capture(manifest: &Manifest, table: &TableSpec) -> TriggerSql {
    let name = format!("sync_capture_{}_delete", table.name);
    let id = table.id.0;
    let key = table.key_column;
    let published = if publication::condition_over(manifest, table, "OLD").is_some() {
        format!(
            "\n    AND (EXISTS (SELECT 1 FROM sync_rows WHERE table_id = {id} AND row_key = OLD.{key})\n        OR EXISTS (SELECT 1 FROM sync_capture WHERE table_id = {id} AND row_key = OLD.{key}))"
        )
    } else {
        String::new()
    };
    let mut sql = header(&name, "AFTER", "DELETE", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}{published}\nBEGIN\n    {CAPTURE_INSERT}\n    VALUES ({id}, OLD.{key}, 0, 0, 1, {NOW_MS})\n    ON CONFLICT (table_id, row_key) DO UPDATE SET deleted = 1;\nEND",
    );
    TriggerSql { name, sql }
}

/// Refuses a local update that makes a row satisfying its publication predicate fail it, so
/// a published row never becomes a draft again.
fn revert_guard(table: &TableSpec) -> Option<TriggerSql> {
    let old = publication::own_over(table, "OLD")?;
    let new = publication::own_over(table, "NEW")?;
    let name = format!("sync_publish_{}", table.name);
    let mut sql = header(&name, "BEFORE", "UPDATE", table.name);
    let _ = write!(
        sql,
        "WHEN {NOT_APPLYING}\n    AND {old}\n    AND NOT {new}\n{}",
        abort_body("A published row cannot become a draft again")
    );
    Some(TriggerSql { name, sql })
}

/// Alias of the owning row in the publication condition of an owned child capture.
const OWNER_ALIAS: &str = "sync_owner";

/// Whether the owning row of `parent_key` is published, for a parent table whose rows publish
/// when `published` holds over [`OWNER_ALIAS`].
fn owner_published(parent: &TableSpec, parent_key: &str, published: &str) -> String {
    format!(
        "EXISTS (SELECT 1 FROM {table} AS {OWNER_ALIAS} WHERE {OWNER_ALIAS}.{key} = {parent_key} AND {published})",
        table = parent.name,
        key = parent.key_column,
    )
}

/// An owned table whose changes capture the group of its owning row.
struct OwnedCapture<'a> {
    /// Replicated table owning the rows.
    parent: &'a TableSpec,
    /// Owned table.
    owned: &'a OwnedTable,
    /// Owned table holding the parent rows of a nested table.
    parent_row: Option<&'a OwnedTable>,
    /// Mask bit of the group.
    bit: &'a str,
    /// Publication condition of the owning row over [`OWNER_ALIAS`].
    published: Option<&'a str>,
}

/// Conditions under which a deleted owned row is a foreign key action rather than an edit of
/// the group: its referenced row was deleted, or its parent owned row was. The deletion that
/// caused it is captured on its own, and a row whose target is gone stays in the group value.
fn owned_delete_conditions(manifest: &Manifest, capture: &OwnedCapture<'_>) -> String {
    let mut conditions = String::new();
    for reference in capture.owned.references {
        let target = manifest
            .table(reference.table)
            .unwrap_or_else(|| panic!("{} references an unknown table", capture.owned.table));
        let _ = write!(
            conditions,
            "\n    AND NOT (OLD.{column} IS NOT NULL AND NOT EXISTS (SELECT 1 FROM {target_table} WHERE {target_key} = OLD.{column}))",
            column = reference.column,
            target_table = target.name,
            target_key = target.key_column,
        );
    }
    if let (Some(parent_row), OwnedOrder::Nested { parent_ordinal, .. }) =
        (capture.parent_row, capture.owned.order)
        && let OwnedOrder::Ordinal(ordinal) = parent_row.order
    {
        let _ = write!(
            conditions,
            "\n    AND EXISTS (SELECT 1 FROM {table} WHERE {owner} = OLD.{column} AND {ordinal} = OLD.{parent_ordinal})",
            table = parent_row.table,
            owner = parent_row.parent_column,
            column = capture.owned.parent_column,
        );
    }
    conditions
}

/// Captures the owning row of a changed owned row. While the owning row is a draft, nothing
/// is captured; the capture that publishes it carries every group.
fn owned_capture(manifest: &Manifest, capture: &OwnedCapture<'_>, event: Event) -> TriggerSql {
    let child = capture.owned.table;
    let parent_column = capture.owned.parent_column;
    let bit = capture.bit;
    let parent = capture.parent;
    let name = format!("sync_capture_{child}_{}", event.suffix());
    let id = parent.id.0;
    let body = match (event, capture.published) {
        (Event::Insert | Event::Delete, None) => {
            let row = if event == Event::Insert { "NEW" } else { "OLD" };
            format!(
                "    {CAPTURE_INSERT}\n    VALUES ({id}, {row}.{parent_column}, {bit}, 0, 0, {NOW_MS})\n    {MERGE_MASK};"
            )
        }
        (Event::Insert | Event::Delete, Some(published)) => {
            let row = if event == Event::Insert { "NEW" } else { "OLD" };
            let key = format!("{row}.{parent_column}");
            format!(
                "    {CAPTURE_INSERT}\n    SELECT {id}, {key}, {bit}, 0, 0, {NOW_MS}\n    WHERE {}\n    {MERGE_MASK};",
                owner_published(parent, &key, published)
            )
        }
        (Event::Update, None) => format!(
            "    {CAPTURE_INSERT}\n    SELECT {id}, parent.key, {bit}, 0, 0, {NOW_MS}\n    FROM (SELECT NEW.{parent_column} AS key UNION SELECT OLD.{parent_column}) AS parent\n    WHERE true\n    {MERGE_MASK};"
        ),
        (Event::Update, Some(published)) => format!(
            "    {CAPTURE_INSERT}\n    SELECT {id}, parent.key, {bit}, 0, 0, {NOW_MS}\n    FROM (SELECT NEW.{parent_column} AS key UNION SELECT OLD.{parent_column}) AS parent\n    WHERE {}\n    {MERGE_MASK};",
            owner_published(parent, "parent.key", published)
        ),
    };
    let conditions = if event == Event::Delete {
        owned_delete_conditions(manifest, capture)
    } else {
        String::new()
    };
    let mut sql = header(&name, "AFTER", event.keyword(), child);
    let _ = write!(sql, "WHEN {NOT_APPLYING}{conditions}\nBEGIN\n{body}\nEND");
    TriggerSql { name, sql }
}

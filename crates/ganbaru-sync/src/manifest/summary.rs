//! The canonical summary of a manifest: everything that decides how operations validate, merge,
//! and materialize on every replica, and nothing that stays on one device.
//!
//! A manifest version is fixed by its summary. Adapter names, hooks, presentation, replica write
//! flags, `Local` tables and columns, adapter-derived and hook columns, revision bumps, group
//! names, delete holds, and domain triggers are device or diagnostic details, so they are left
//! out.

use std::fmt::Write as _;

use super::{
    ColumnType, Manifest, MergeKind, OnDelete, OwnedCodec, OwnedOrder, OwnedTable, Resolution,
};

impl Manifest {
    /// The canonical summary text, one line per table, group, and resolution.
    pub fn summary(&self) -> String {
        let mut text = String::new();
        let _ = writeln!(text, "manifest {}", self.version);
        for table in self.tables {
            let _ = writeln!(
                text,
                "table {} {} replicated key {}{}",
                table.id.0,
                table.name,
                table.key_column,
                if table.redirects { " redirects" } else { "" }
            );
            if let Some(derived) = table.derived_key {
                let _ = writeln!(
                    text,
                    "  derived key exists {} inputs {}{}",
                    derived.exists_group.get(),
                    derived.key_inputs.join(","),
                    derived
                        .prefix
                        .map_or_else(String::new, |prefix| format!(" prefix {prefix}"))
                );
            }
            if let Some(tree) = table.tree {
                let _ = writeln!(
                    text,
                    "  tree parent {} position {} depth {}",
                    tree.parent_group.get(),
                    tree.position_group.get(),
                    tree.max_depth
                        .map_or_else(|| "unbounded".to_string(), |depth| depth.to_string())
                );
            }
            if let Some(acyclic) = table.acyclic {
                let _ = writeln!(
                    text,
                    "  acyclic {}->{}",
                    acyclic.from_column, acyclic.to_column
                );
            }
            if let Some(predicate) = table.publish_when {
                let _ = writeln!(text, "  publish when {predicate}");
            }
            if let Some(with) = table.published_with {
                let _ = writeln!(text, "  published with {} {}", with.column, with.parent.0);
            }
            for group in table.groups {
                let mut line = format!("  group {} {}", group.id.get(), kind(group.kind));
                if group.surfaced {
                    line.push_str(" surfaced");
                }
                if group.recoverable {
                    line.push_str(" recoverable");
                }
                for column in group.columns() {
                    let _ = write!(line, " {}:{}", column.name, column_type(column.ty));
                    if let Some(reference) = group
                        .references
                        .iter()
                        .find(|reference| reference.column == column.name)
                    {
                        let _ = write!(
                            line,
                            "->{}:{}",
                            reference.table.0,
                            on_delete(reference.on_delete)
                        );
                    }
                }
                let _ = writeln!(text, "{line}");
                for owned in group.owned_tables() {
                    owned_lines(&mut text, owned, "    ");
                }
            }
            for resolution in table.resolutions {
                let _ = match resolution {
                    Resolution::DuplicateRepair { columns, collation } => writeln!(
                        text,
                        "  unique {} {}",
                        columns.join(","),
                        collation.sql_name().to_ascii_lowercase()
                    ),
                    Resolution::WithinGroup { columns, group } => {
                        writeln!(text, "  check {} within {}", columns.join(","), group.get())
                    }
                };
            }
        }
        text
    }
}

/// One line per owned table, its nested tables indented below it.
fn owned_lines(text: &mut String, owned: &OwnedTable, indent: &str) {
    let order = match owned.order {
        OwnedOrder::Ordinal(ordinal) => format!("ordinal {ordinal}"),
        OwnedOrder::Nested {
            parent_ordinal,
            ordinal,
        } => format!("nested {parent_ordinal},{ordinal}"),
        OwnedOrder::Key(columns) => format!("key {}", columns.join(",")),
    };
    let codec = match owned.codec {
        OwnedCodec::Generic => "generic",
        OwnedCodec::Custom => "custom",
    };
    let mut line = format!(
        "{indent}owned {} parent {} {order} rows {} codec {codec}",
        owned.table, owned.parent_column, owned.max_rows
    );
    for column in owned.columns {
        let _ = write!(line, " {}:{}", column.name, column_type(column.ty));
        if let Some(reference) = owned
            .references
            .iter()
            .find(|reference| reference.column == column.name)
        {
            let _ = write!(line, "->{}:cascade", reference.table.0);
        }
    }
    if !owned.unique.is_empty() {
        let _ = write!(line, " unique {}", owned.unique.join(","));
    }
    let _ = writeln!(text, "{line}");
    let nested_indent = format!("{indent}  ");
    for nested in owned.nested {
        owned_lines(text, nested, &nested_indent);
    }
}

fn kind(kind: MergeKind) -> String {
    match kind {
        MergeKind::Immutable => "immutable".to_string(),
        MergeKind::Register => "register".to_string(),
        MergeKind::Coupled => "coupled".to_string(),
        MergeKind::Max => "max".to_string(),
        MergeKind::Position => "position".to_string(),
        MergeKind::Reference {
            table,
            on_delete: action,
        } => {
            format!("reference {} {}", table.0, on_delete(action))
        }
    }
}

const fn on_delete(action: OnDelete) -> &'static str {
    match action {
        OnDelete::SetNull => "set_null",
        OnDelete::Cascade => "cascade",
    }
}

fn column_type(ty: ColumnType) -> String {
    match ty {
        ColumnType::Text { max_chars } => format!("text(..={max_chars})"),
        ColumnType::OptionalText { max_chars } => format!("optional_text(..={max_chars})"),
        ColumnType::TrimmedText {
            min_chars,
            max_chars,
        } => format!("trimmed_text({min_chars}..={max_chars})"),
        ColumnType::Integer { min, max } => format!("integer({min}..={max})"),
        ColumnType::OptionalInteger { min, max } => format!("optional_integer({min}..={max})"),
        ColumnType::Flag => "flag".to_string(),
        ColumnType::Timestamp => "timestamp".to_string(),
        ColumnType::OptionalTimestamp => "optional_timestamp".to_string(),
        ColumnType::OrderKey => "order_key".to_string(),
        ColumnType::OptionalRowKey => "optional_row_key".to_string(),
    }
}

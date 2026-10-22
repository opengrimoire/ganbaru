//! The canonical summary of a manifest: everything that decides how operations validate, merge,
//! and materialize on every replica, and nothing that stays on one device.
//!
//! A manifest version is fixed by its summary. Adapter names, hooks, presentation, replica write
//! flags, `Local` tables and columns, adapter-derived columns, revision bumps, group names, and
//! domain triggers are device or diagnostic details, so they are left out.

use std::fmt::Write as _;

use super::{ColumnType, GroupStorage, Manifest, MergeKind, Resolution};

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
            for group in table.groups {
                let mut line = format!("  group {} {}", group.id.get(), kind(group.kind));
                if group.surfaced {
                    line.push_str(" surfaced");
                }
                if group.recoverable {
                    line.push_str(" recoverable");
                }
                match group.storage {
                    GroupStorage::Columns(columns) => {
                        for column in columns {
                            let _ = write!(line, " {}:{}", column.name, column_type(column.ty));
                        }
                    }
                    GroupStorage::Owned(owned) => {
                        let _ = write!(
                            line,
                            " owned {} parent {} order {} columns {}",
                            owned.table,
                            owned.parent_column,
                            owned.order_column,
                            owned.columns.join(",")
                        );
                    }
                }
                let _ = writeln!(text, "{line}");
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

fn kind(kind: MergeKind) -> String {
    match kind {
        MergeKind::Immutable => "immutable".to_string(),
        MergeKind::Register => "register".to_string(),
        MergeKind::Coupled => "coupled".to_string(),
        MergeKind::Max => "max".to_string(),
        MergeKind::Position => "position".to_string(),
        MergeKind::Reference { table } => format!("reference {}", table.0),
    }
}

fn column_type(ty: ColumnType) -> String {
    match ty {
        ColumnType::Text { max_chars } => format!("text(..={max_chars})"),
        ColumnType::TrimmedText {
            min_chars,
            max_chars,
        } => format!("trimmed_text({min_chars}..={max_chars})"),
        ColumnType::Integer { min, max } => format!("integer({min}..={max})"),
        ColumnType::Flag => "flag".to_string(),
        ColumnType::Timestamp => "timestamp".to_string(),
        ColumnType::OptionalTimestamp => "optional_timestamp".to_string(),
        ColumnType::OrderKey => "order_key".to_string(),
        ColumnType::OptionalRowKey => "optional_row_key".to_string(),
    }
}

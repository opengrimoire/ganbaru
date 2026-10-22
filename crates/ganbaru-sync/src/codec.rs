//! Owned child rows as blob fields of group values.
//!
//! The generic codec writes a big-endian `u16` row count, then each row: its data columns as a
//! value encoding, followed by the rows of each nested table in declaration order, written the
//! same way. Decoding refuses a count above the table's bound, a field that breaks its column
//! rule, a value repeated in a unique column within the blob, and trailing bytes, so every
//! accepted blob encodes back to the same bytes. A custom codec belongs to the table's domain
//! adapter; its rows pass the same column, count, and uniqueness rules.

use std::collections::{BTreeSet, HashSet};

use ganbaru_sync_contracts::codec::{Reader, Writer};
use ganbaru_sync_contracts::{Field, TableId, Value};

use crate::Engine;
use crate::manifest::{GroupSpec, OwnedCodec, OwnedTable, TableSpec};
use crate::validate::{Invalid, column_value};

/// One owned child row: its data columns in [`OwnedTable::columns`] order and, per nested
/// table, the nested rows that belong to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedRow {
    /// Data column fields.
    pub fields: Vec<Field>,
    /// Rows of each nested table in declaration order.
    pub nested: Vec<Vec<OwnedRow>>,
}

impl OwnedRow {
    /// A row without nested tables.
    pub fn flat(fields: Vec<Field>) -> Self {
        Self {
            fields,
            nested: Vec::new(),
        }
    }
}

/// Checks rows against the table's column rules, row bound, nested shape, and unique columns.
pub fn check_rows(owned: &OwnedTable, rows: &[OwnedRow]) -> Result<(), Invalid> {
    let mut seen = HashSet::new();
    check_level(owned, rows, &mut seen)
}

/// Unique values already seen in a blob, by table and column.
type Seen<'a> = HashSet<(&'static str, &'static str, &'a Field)>;

fn check_level<'a>(
    owned: &OwnedTable,
    rows: &'a [OwnedRow],
    seen: &mut Seen<'a>,
) -> Result<(), Invalid> {
    if rows.len() > owned.max_rows {
        return Err("owned row count");
    }
    for row in rows {
        if row.fields.len() != owned.columns.len() {
            return Err("owned row fields");
        }
        for (column, field) in owned.columns.iter().zip(&row.fields) {
            if !column_value(column.ty, field) {
                return Err("owned column value");
            }
            // SQLite unique indexes admit any number of nulls.
            if owned.unique.contains(&column.name)
                && *field != Field::Null
                && !seen.insert((owned.table, column.name, field))
            {
                return Err("owned unique value");
            }
        }
        let nested_shape = if owned.nested.is_empty() {
            row.nested.is_empty()
        } else {
            row.nested.len() == owned.nested.len()
        };
        if !nested_shape {
            return Err("owned nested rows");
        }
        for (nested, nested_rows) in owned.nested.iter().zip(&row.nested) {
            check_level(nested, nested_rows, seen)?;
        }
    }
    Ok(())
}

/// Encodes rows with the generic codec after checking them.
pub fn encode(owned: &OwnedTable, rows: &[OwnedRow]) -> Result<Vec<u8>, Invalid> {
    check_rows(owned, rows)?;
    let mut writer = Writer::default();
    write_rows(&mut writer, owned, rows)?;
    Ok(writer.into_bytes())
}

fn write_rows(writer: &mut Writer, owned: &OwnedTable, rows: &[OwnedRow]) -> Result<(), Invalid> {
    let count = u16::try_from(rows.len()).map_err(|_| "owned row count")?;
    writer.u16(count);
    for row in rows {
        Value::new(row.fields.clone())
            .map_err(|_| "owned row bounds")?
            .encode_into(writer);
        for (nested, nested_rows) in owned.nested.iter().zip(&row.nested) {
            write_rows(writer, nested, nested_rows)?;
        }
    }
    Ok(())
}

/// Decodes and checks a generic codec blob.
pub fn decode(owned: &OwnedTable, bytes: &[u8]) -> Result<Vec<OwnedRow>, Invalid> {
    let mut reader = Reader::new(bytes);
    let rows = read_rows(&mut reader, owned)?;
    reader.finish().map_err(|_| "owned encoding")?;
    check_rows(owned, &rows)?;
    Ok(rows)
}

fn read_rows(reader: &mut Reader<'_>, owned: &OwnedTable) -> Result<Vec<OwnedRow>, Invalid> {
    let count = usize::from(reader.u16().map_err(|_| "owned encoding")?);
    if count > owned.max_rows {
        return Err("owned row count");
    }
    let mut rows = Vec::with_capacity(count.min(reader.remaining()));
    for _ in 0..count {
        let fields = Value::decode_from(reader)
            .map_err(|_| "owned encoding")?
            .into_fields();
        let nested = owned
            .nested
            .iter()
            .map(|nested| read_rows(reader, nested))
            .collect::<Result<Vec<_>, _>>()?;
        rows.push(OwnedRow { fields, nested });
    }
    Ok(rows)
}

/// Encodes the rows of an owned table of `table` with the table's codec.
pub(crate) fn encode_table(
    engine: &Engine,
    table: &TableSpec,
    owned: &OwnedTable,
    rows: Vec<OwnedRow>,
) -> Result<Vec<u8>, Invalid> {
    match owned.codec {
        OwnedCodec::Generic => encode(owned, &rows),
        OwnedCodec::Custom => {
            check_rows(owned, &rows)?;
            let adapter = engine.adapter(table).ok_or("owned table without adapter")?;
            let fields = rows.into_iter().map(|row| row.fields).collect();
            adapter
                .encode_owned(table.id, owned.table, fields)
                .map_err(|error| error.0)
        }
    }
}

/// Decodes and checks the blob of an owned table of `table` with the table's codec.
pub(crate) fn decode_table(
    engine: &Engine,
    table: &TableSpec,
    owned: &OwnedTable,
    bytes: &[u8],
) -> Result<Vec<OwnedRow>, Invalid> {
    match owned.codec {
        OwnedCodec::Generic => decode(owned, bytes),
        OwnedCodec::Custom => {
            let adapter = engine.adapter(table).ok_or("owned table without adapter")?;
            let rows: Vec<OwnedRow> = adapter
                .decode_owned(table.id, owned.table, bytes)
                .map_err(|error| error.0)?
                .into_iter()
                .map(OwnedRow::flat)
                .collect();
            check_rows(owned, &rows)?;
            Ok(rows)
        }
    }
}

/// The blob of the owned table at `index` among a group value's owned tables.
pub(crate) fn owned_blob<'a>(
    group: &GroupSpec,
    value: &'a Value,
    index: usize,
) -> Result<&'a [u8], Invalid> {
    match value.fields().get(group.columns().len() + index) {
        Some(Field::Blob(bytes)) => Ok(bytes),
        _ => Err("owned group value"),
    }
}

/// Row keys the owned rows of a group value reference: the blob field index, the referenced
/// table, and the key, each once.
pub(crate) fn owned_targets(
    group: &GroupSpec,
    value: &Value,
) -> Result<BTreeSet<(usize, TableId, String)>, Invalid> {
    let mut targets = BTreeSet::new();
    let first = group.columns().len();
    for (index, owned) in group.owned_tables().iter().enumerate() {
        let referencing = std::iter::once(owned)
            .chain(owned.nested.iter())
            .any(|table| !table.references.is_empty());
        if !referencing {
            continue;
        }
        // Only the generic codec carries references.
        let rows = decode(owned, owned_blob(group, value, index)?)?;
        collect_targets(owned, &rows, first + index, &mut targets);
    }
    Ok(targets)
}

fn collect_targets(
    owned: &OwnedTable,
    rows: &[OwnedRow],
    field: usize,
    targets: &mut BTreeSet<(usize, TableId, String)>,
) {
    for row in rows {
        for (column, reference) in owned.reference_fields() {
            if let Some(Field::Text(key)) = row.fields.get(column) {
                targets.insert((field, reference.table, key.clone()));
            }
        }
        for (nested, nested_rows) in owned.nested.iter().zip(&row.nested) {
            collect_targets(nested, nested_rows, field, targets);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{ColumnSpec, ColumnType, OwnedOrder, OwnedReference};

    const NESTED: OwnedTable = OwnedTable {
        table: "codec_steps",
        parent_column: "owner_id",
        order: OwnedOrder::Nested {
            parent_ordinal: "card_ordinal",
            ordinal: "ordinal",
        },
        columns: &[
            ColumnSpec {
                name: "step_id",
                ty: ColumnType::Text { max_chars: 8 },
            },
            ColumnSpec {
                name: "done",
                ty: ColumnType::Flag,
            },
        ],
        references: &[],
        unique: &["step_id"],
        max_rows: 2,
        codec: OwnedCodec::Generic,
        nested: &[],
    };

    const OWNED: OwnedTable = OwnedTable {
        table: "codec_cards",
        parent_column: "owner_id",
        order: OwnedOrder::Ordinal("ordinal"),
        columns: &[
            ColumnSpec {
                name: "card_id",
                ty: ColumnType::Text { max_chars: 8 },
            },
            ColumnSpec {
                name: "note",
                ty: ColumnType::OptionalText { max_chars: 4 },
            },
            ColumnSpec {
                name: "target_id",
                ty: ColumnType::OptionalRowKey,
            },
        ],
        references: &[OwnedReference {
            column: "target_id",
            table: TableId(7),
        }],
        unique: &["card_id"],
        max_rows: 3,
        codec: OwnedCodec::Generic,
        nested: &[NESTED],
    };

    fn text(value: &str) -> Field {
        Field::Text(value.to_string())
    }

    fn step(id: &str, done: i64) -> OwnedRow {
        OwnedRow::flat(vec![text(id), Field::Integer(done)])
    }

    fn card(id: &str, note: Field, target: Field, steps: Vec<OwnedRow>) -> OwnedRow {
        OwnedRow {
            fields: vec![text(id), note, target],
            nested: vec![steps],
        }
    }

    fn sample() -> Vec<OwnedRow> {
        vec![
            card(
                "a",
                text("hi"),
                text("t1"),
                vec![step("s1", 1), step("s2", 0)],
            ),
            card("b", Field::Null, Field::Null, Vec::new()),
            card("c", text("ñü"), text("t1"), vec![step("s3", 0)]),
        ]
    }

    #[test]
    fn codec_round_trips_nested_rows_byte_exactly() {
        let rows = sample();
        let bytes = encode(&OWNED, &rows).unwrap();
        let decoded = decode(&OWNED, &bytes).unwrap();
        assert_eq!(decoded, rows);
        assert_eq!(encode(&OWNED, &decoded).unwrap(), bytes);
        assert_eq!(encode(&OWNED, &[]).unwrap(), vec![0, 0]);
        assert!(bytes.len() <= OWNED.max_encoded_bytes());
    }

    #[test]
    fn codec_refuses_rows_beyond_the_bounds_of_each_level() {
        let mut rows = sample();
        rows.push(card("d", Field::Null, Field::Null, Vec::new()));
        assert_eq!(encode(&OWNED, &rows), Err("owned row count"));

        let mut rows = sample();
        rows[1].nested[0] = vec![step("x", 0), step("y", 0), step("z", 0)];
        assert_eq!(encode(&OWNED, &rows), Err("owned row count"));

        let mut bytes = encode(&OWNED, &[]).unwrap();
        bytes[1] = 4;
        assert_eq!(decode(&OWNED, &bytes), Err("owned row count"));
    }

    #[test]
    fn codec_refuses_values_that_break_column_rules() {
        let mut rows = sample();
        rows[0].fields[1] = text("toolong");
        assert_eq!(encode(&OWNED, &rows), Err("owned column value"));

        let mut rows = sample();
        rows[0].nested[0][0].fields[1] = Field::Integer(2);
        assert_eq!(encode(&OWNED, &rows), Err("owned column value"));

        let mut rows = sample();
        rows[0].fields.pop();
        assert_eq!(encode(&OWNED, &rows), Err("owned row fields"));

        let mut rows = sample();
        rows[1].nested.clear();
        assert_eq!(encode(&OWNED, &rows), Err("owned nested rows"));
    }

    #[test]
    fn codec_refuses_repeated_unique_values_within_the_blob() {
        let mut rows = sample();
        rows[2].fields[0] = text("a");
        assert_eq!(encode(&OWNED, &rows), Err("owned unique value"));

        // Nested ids are unique across every parent row of the owner.
        let mut rows = sample();
        rows[2].nested[0][0].fields[0] = text("s1");
        assert_eq!(encode(&OWNED, &rows), Err("owned unique value"));
    }

    #[test]
    fn codec_refuses_truncated_trailing_and_malformed_bytes() {
        let bytes = encode(&OWNED, &sample()).unwrap();
        assert_eq!(
            decode(&OWNED, &bytes[..bytes.len() - 1]),
            Err("owned encoding")
        );
        let mut trailing = bytes.clone();
        trailing.push(0);
        assert_eq!(decode(&OWNED, &trailing), Err("owned encoding"));
        let mut tag = bytes;
        // The first field tag of the first row follows the count and the field count.
        tag[3] = 9;
        assert_eq!(decode(&OWNED, &tag), Err("owned encoding"));
    }

    #[test]
    fn codec_reports_each_referenced_key_once() {
        let rows = sample();
        let mut targets = BTreeSet::new();
        collect_targets(&OWNED, &rows, 2, &mut targets);
        assert_eq!(
            targets.into_iter().collect::<Vec<_>>(),
            vec![(2, TableId(7), "t1".to_string())]
        );
    }
}

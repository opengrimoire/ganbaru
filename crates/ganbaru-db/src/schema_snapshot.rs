//! Committed reference catalog for the complete embedded migration chain.
//!
//! Building the reference catalog runs every migration on an empty database, which costs
//! seconds even in release builds because SQLite reparses the whole schema after each
//! `ALTER TABLE`. Schema validation instead reads the catalog that the full chain produces from
//! `schema-catalog.txt` beside the crate manifest. The file lists the identity of every
//! migration it was built from, so it is used only while the embedded chain is exactly that
//! chain, and a test rebuilds the catalog from the real migrations and requires an exact match.
//!
//! The file starts with [`HEADER`], then one `migration` line per up migration with its version
//! and hexadecimal checksum, then one `object` line per `sqlite_schema` row in catalog order.
//! Fields are separated by tabs. Backslash, tab, line feed, and carriage return are escaped as
//! `\\`, `\t`, `\n`, and `\r`, and a missing SQL text is the field `\N`, which no escaped string
//! can equal.

use crate::{SchemaCatalog, SchemaObject, up_migrations};

/// Committed catalog of an empty database migrated through the full embedded chain.
const COMMITTED_SNAPSHOT: &str = include_str!("../schema-catalog.txt");

/// First line of a snapshot, naming its format version.
const HEADER: &str = "ganbaru-db reference schema catalog v1";

/// Field that stands for a missing SQL text.
const NULL_FIELD: &str = "\\N";

const MIGRATION_TAG: &str = "migration";
const OBJECT_TAG: &str = "object";

/// Version and hexadecimal checksum of one up migration.
type MigrationKey = (i64, String);

/// A parsed snapshot: the migration chain it was built from and the catalog that chain produced.
#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    migrations: Vec<MigrationKey>,
    catalog: SchemaCatalog,
}

/// Returns the committed catalog when it was built from exactly the embedded migration chain.
///
/// A snapshot for a different chain, or one that does not parse, yields `None` so the caller
/// builds the catalog from the migrations instead.
pub(crate) fn committed_catalog() -> Option<SchemaCatalog> {
    catalog_for_embedded_chain(COMMITTED_SNAPSHOT)
}

fn catalog_for_embedded_chain(text: &str) -> Option<SchemaCatalog> {
    let snapshot = parse(text).ok()?;
    (snapshot.migrations == embedded_chain()).then_some(snapshot.catalog)
}

/// Returns the version and checksum of every embedded up migration in order.
fn embedded_chain() -> Vec<MigrationKey> {
    up_migrations()
        .map(|migration| (migration.version, hex(migration.checksum.as_ref())))
        .collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// Renders the snapshot of `catalog` as built from the embedded migration chain.
#[cfg(test)]
fn render(catalog: &SchemaCatalog) -> String {
    let mut text = String::new();
    text.push_str(HEADER);
    text.push('\n');
    for (version, checksum) in embedded_chain() {
        text.push_str(&format!("{MIGRATION_TAG}\t{version}\t{checksum}\n"));
    }
    for (kind, name, table, sql) in catalog {
        let sql = sql
            .as_deref()
            .map_or_else(|| NULL_FIELD.to_string(), escape);
        text.push_str(&format!(
            "{OBJECT_TAG}\t{}\t{}\t{}\t{sql}\n",
            escape(kind),
            escape(name),
            escape(table),
        ));
    }
    text
}

fn parse(text: &str) -> Result<Snapshot, String> {
    // A checkout that converts line endings adds a carriage return to each line; content
    // carriage returns are always escaped, so a trailing one is never part of a field.
    let mut lines = text
        .lines()
        .map(|line| line.strip_suffix('\r').unwrap_or(line));
    if lines.next() != Some(HEADER) {
        return Err("schema catalog snapshot has an unknown header".to_string());
    }
    let mut snapshot = Snapshot {
        migrations: Vec::new(),
        catalog: SchemaCatalog::new(),
    };
    for (index, line) in lines.enumerate() {
        let fields = line.split('\t').collect::<Vec<_>>();
        let parsed = match fields.as_slice() {
            [MIGRATION_TAG, version, checksum] if snapshot.catalog.is_empty() => version
                .parse::<i64>()
                .map_err(|error| format!("migration version: {error}"))
                .map(|version| snapshot.migrations.push((version, (*checksum).to_string()))),
            [OBJECT_TAG, kind, name, table, sql] => {
                parse_object(kind, name, table, sql).map(|object| {
                    snapshot.catalog.insert(object);
                })
            }
            _ => Err("unexpected line".to_string()),
        };
        parsed.map_err(|error| format!("schema catalog snapshot line {}: {error}", index + 2))?;
    }
    Ok(snapshot)
}

fn parse_object(kind: &str, name: &str, table: &str, sql: &str) -> Result<SchemaObject, String> {
    let sql = if sql == NULL_FIELD {
        None
    } else {
        Some(unescape(sql)?)
    };
    Ok((unescape(kind)?, unescape(name)?, unescape(table)?, sql))
}

#[cfg(test)]
fn escape(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for character in value.chars() {
        match character {
            '\\' => escaped.push_str("\\\\"),
            '\t' => escaped.push_str("\\t"),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            other => escaped.push(other),
        }
    }
    escaped
}

fn unescape(value: &str) -> Result<String, String> {
    let mut unescaped = String::with_capacity(value.len());
    let mut characters = value.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            unescaped.push(character);
            continue;
        }
        match characters.next() {
            Some('\\') => unescaped.push('\\'),
            Some('t') => unescaped.push('\t'),
            Some('n') => unescaped.push('\n'),
            Some('r') => unescaped.push('\r'),
            _ => return Err(format!("invalid escape in {value:?}")),
        }
    }
    Ok(unescaped)
}

#[cfg(test)]
mod tests {
    use super::{
        COMMITTED_SNAPSHOT, HEADER, catalog_for_embedded_chain, committed_catalog, parse, render,
    };
    use crate::SchemaCatalog;

    /// Environment variable that makes the snapshot test rewrite the committed file.
    const REGENERATE_VARIABLE: &str = "GANBARU_WRITE_SCHEMA_CATALOG";

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("create snapshot test runtime")
            .block_on(future)
    }

    fn object(kind: &str, name: &str, table: &str, sql: Option<&str>) -> crate::SchemaObject {
        (
            kind.to_string(),
            name.to_string(),
            table.to_string(),
            sql.map(str::to_string),
        )
    }

    /// Rebuilds the catalog from the real migration chain and requires the committed snapshot
    /// to equal it. With `GANBARU_WRITE_SCHEMA_CATALOG=1` set, the test writes the snapshot
    /// instead, which is how a new migration updates it.
    #[test]
    fn committed_snapshot_matches_the_migration_chain() {
        let built = block_on(async {
            crate::build_reference_catalog(crate::latest_migration_version().unwrap())
                .await
                .unwrap()
        });
        let rendered = render(&built);
        if std::env::var_os(REGENERATE_VARIABLE).is_some() {
            let path = concat!(env!("CARGO_MANIFEST_DIR"), "/schema-catalog.txt");
            std::fs::write(path, &rendered).unwrap();
            return;
        }
        let regenerate = format!(
            "crates/ganbaru-db/schema-catalog.txt is stale. Regenerate it after changing \
             migrations with `{REGENERATE_VARIABLE}=1 cargo test -p ganbaru-db --lib \
             committed_snapshot_matches_the_migration_chain` and commit the result."
        );
        assert_eq!(committed_catalog().as_ref(), Some(&built), "{regenerate}");
        assert_eq!(
            parse(COMMITTED_SNAPSHOT).unwrap(),
            parse(&rendered).unwrap(),
            "{regenerate}"
        );
    }

    #[test]
    fn snapshot_round_trips_fields_that_need_escaping() {
        let catalog = SchemaCatalog::from([
            object("table", "plain", "plain", Some("CREATE TABLE plain (id)")),
            object(
                "table",
                "tab\tand\\slash",
                "line\nbreak",
                Some("CREATE TABLE t (\r\n  a TEXT DEFAULT '\\N'\t)"),
            ),
            object("index", "sqlite_autoindex_plain_1", "plain", None),
            object("trigger", "empty", "plain", Some("")),
            object("view", "\\N", "\\", Some("\\N")),
        ]);
        let rendered = render(&catalog);
        assert_eq!(parse(&rendered).unwrap().catalog, catalog);
        assert_eq!(render(&parse(&rendered).unwrap().catalog), rendered);
        assert_eq!(catalog_for_embedded_chain(&rendered), Some(catalog.clone()));
        let windows = rendered.replace('\n', "\r\n");
        assert_eq!(catalog_for_embedded_chain(&windows), Some(catalog));
    }

    #[test]
    fn snapshot_for_another_migration_chain_is_not_used() {
        let catalog = SchemaCatalog::from([object("table", "a", "a", Some("CREATE TABLE a (x)"))]);
        let rendered = render(&catalog);
        let first_migration = rendered.lines().nth(1).unwrap().to_string();
        let mut changed_checksum = first_migration.clone();
        let last = changed_checksum.pop().unwrap();
        changed_checksum.push(if last == '0' { '1' } else { '0' });
        for stale in [
            rendered.replacen(&first_migration, &changed_checksum, 1),
            rendered.replacen(&format!("{first_migration}\n"), "", 1),
            rendered.replacen(
                &format!("{first_migration}\n"),
                &format!("{first_migration}\n{first_migration}\n"),
                1,
            ),
        ] {
            assert!(parse(&stale).is_ok());
            assert_eq!(catalog_for_embedded_chain(&stale), None);
        }
    }

    #[test]
    fn malformed_snapshots_are_rejected() {
        let valid = render(&SchemaCatalog::from([object(
            "table",
            "a",
            "a",
            Some("CREATE TABLE a (x)"),
        )]));
        let migration = valid.lines().nth(1).unwrap();
        for malformed in [
            String::new(),
            valid.replacen(HEADER, "ganbaru-db reference schema catalog v0", 1),
            format!("{valid}object\ttable\tb\tb\n"),
            format!("{valid}object\ttable\tb\tb\tCREATE TABLE b (\\x)\n"),
            format!("{valid}object\ttable\tb\tb\ttrailing\\\n"),
            format!("{valid}{migration}\n"),
            format!("{valid}unknown\tline\n"),
            valid.replacen("migration\t", "migration\tnot-a-number\t", 1),
        ] {
            assert!(parse(&malformed).is_err(), "{malformed:?}");
            assert_eq!(catalog_for_embedded_chain(&malformed), None);
        }
    }
}

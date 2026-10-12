//! The vault manifest against a freshly migrated database.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::SqlitePool;

use super::{block_on, migrated_pool};
use crate::manifest::vault::VAULT_MANIFEST;
use crate::manifest::{
    Classification, Collation, GroupStorage, Manifest, MergeKind, Resolution, TableSpec,
};
use crate::triggers;

async fn schema_tables(pool: &SqlitePool) -> Vec<(String, String)> {
    sqlx::query_as(
        "SELECT name, type FROM pragma_table_list
         WHERE schema = 'main' AND name NOT LIKE 'sqlite\\_%' ESCAPE '\\'
             AND name <> '_sqlx_migrations'
         ORDER BY name",
    )
    .fetch_all(pool)
    .await
    .unwrap()
}

async fn table_columns(pool: &SqlitePool, table: &str) -> Vec<(String, String, i64)> {
    sqlx::query_as("SELECT name, type, pk FROM pragma_table_info(?) ORDER BY cid")
        .bind(table)
        .fetch_all(pool)
        .await
        .unwrap()
}

/// `(from, table, to, on_delete)` for every foreign key column of a table.
async fn foreign_keys(pool: &SqlitePool, table: &str) -> Vec<(String, String, String, String)> {
    sqlx::query_as(
        "SELECT \"from\", \"table\", coalesce(\"to\", ''), on_delete
         FROM pragma_foreign_key_list(?)",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap()
}

#[test]
fn every_schema_table_is_classified() {
    block_on(async {
        let pool = migrated_pool().await;
        let tables = schema_tables(&pool).await;
        let types = tables.iter().cloned().collect::<BTreeMap<_, _>>();
        let manifest = &VAULT_MANIFEST;

        for table in manifest.tables {
            assert_eq!(types.get(table.name).map(String::as_str), Some("table"));
            for (_, owned) in table.owned_children() {
                let child = owned.table;
                assert_eq!(types.get(child).map(String::as_str), Some("table"));
                assert_eq!(manifest.classify(child), Classification::OwnedChildren);
            }
            assert_eq!(manifest.classify(table.name), Classification::Replicated);
        }
        for derived in manifest.derived {
            assert!(
                matches!(
                    types.get(*derived).map(String::as_str),
                    Some("table" | "virtual")
                ),
                "derived table {derived} is missing"
            );
        }

        let engine_tables = tables
            .iter()
            .filter(|(name, _)| name.starts_with("sync_"))
            .map(|(name, _)| name.as_str())
            .collect::<BTreeSet<_>>();
        let declared = manifest.engine.iter().copied().collect::<BTreeSet<_>>();
        assert_eq!(
            engine_tables, declared,
            "engine tables must match sync_* tables"
        );

        // Shadow tables follow their virtual table's classification.
        let virtual_tables = tables
            .iter()
            .filter(|(_, kind)| kind == "virtual")
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>();
        for (name, kind) in &tables {
            if kind == "shadow" {
                assert!(
                    virtual_tables
                        .iter()
                        .any(|parent| name.starts_with(&format!("{parent}_"))),
                    "shadow table {name} belongs to no virtual table"
                );
            }
        }
    });
}

#[test]
fn replicated_columns_are_partitioned() {
    block_on(async {
        let pool = migrated_pool().await;
        for table in VAULT_MANIFEST.tables {
            let columns = table_columns(&pool, table.name).await;
            let mut declared = vec![table.key_column];
            declared.extend(
                table
                    .groups
                    .iter()
                    .flat_map(|group| group.columns())
                    .map(|column| column.name),
            );
            declared.extend(table.local_columns);
            declared.extend(table.derived_columns);
            let unique = declared.iter().copied().collect::<BTreeSet<_>>();
            assert_eq!(
                unique.len(),
                declared.len(),
                "{} repeats a column",
                table.name
            );
            let actual = columns
                .iter()
                .map(|(name, _, _)| name.as_str())
                .collect::<BTreeSet<_>>();
            assert_eq!(actual, unique, "{} columns", table.name);

            let primary_key = columns
                .iter()
                .filter(|(_, _, pk)| *pk > 0)
                .map(|(name, kind, _)| (name.as_str(), kind.as_str()))
                .collect::<Vec<_>>();
            assert_eq!(
                primary_key,
                [(table.key_column, "TEXT")],
                "{} key",
                table.name
            );

            for (group, owned) in table.owned_children() {
                let child_columns = table_columns(&pool, owned.table).await;
                let mut declared = vec![owned.parent_column, owned.order_column];
                declared.extend(owned.columns);
                let unique = declared.iter().copied().collect::<BTreeSet<_>>();
                assert_eq!(
                    unique.len(),
                    declared.len(),
                    "{} repeats a column",
                    owned.table
                );
                let actual = child_columns
                    .iter()
                    .map(|(name, _, _)| name.as_str())
                    .collect::<BTreeSet<_>>();
                assert_eq!(
                    actual, unique,
                    "{}.{} child columns",
                    table.name, group.name
                );
                let child_key = child_columns
                    .iter()
                    .filter(|(_, _, pk)| *pk > 0)
                    .map(|(name, _, _)| name.as_str())
                    .collect::<BTreeSet<_>>();
                assert_eq!(
                    child_key,
                    BTreeSet::from([owned.parent_column, owned.order_column]),
                    "{} key",
                    owned.table
                );
            }
        }
    });
}

#[test]
fn foreign_keys_follow_references_and_ownership() {
    block_on(async {
        let pool = migrated_pool().await;
        let manifest = &VAULT_MANIFEST;
        for table in manifest.tables {
            let mut expected = BTreeSet::new();
            for group in table.groups {
                if let MergeKind::Reference { table: target } = group.kind {
                    let target = manifest.table(target).unwrap();
                    let column = group.columns()[0].name;
                    expected.insert((
                        column.to_string(),
                        target.name.to_string(),
                        target.key_column.to_string(),
                        "SET NULL".to_string(),
                    ));
                }
            }
            let actual = foreign_keys(&pool, table.name)
                .await
                .into_iter()
                .collect::<BTreeSet<_>>();
            assert_eq!(actual, expected, "{} foreign keys", table.name);

            for (_, owned) in table.owned_children() {
                let (child, parent_column) = (owned.table, owned.parent_column);
                let child_keys = foreign_keys(&pool, child).await;
                assert_eq!(
                    child_keys,
                    [(
                        parent_column.to_string(),
                        table.name.to_string(),
                        table.key_column.to_string(),
                        "CASCADE".to_string(),
                    )],
                    "{child} foreign keys"
                );
            }
        }
    });
}

#[test]
fn installed_triggers_match_rendering() {
    block_on(async {
        let pool = migrated_pool().await;
        let installed: BTreeMap<String, String> = sqlx::query_as(
            "SELECT name, sql FROM sqlite_schema
             WHERE type = 'trigger' AND name LIKE 'sync\\_%' ESCAPE '\\'",
        )
        .fetch_all(&pool)
        .await
        .unwrap()
        .into_iter()
        .map(|(name, sql): (String, String)| (name, triggers::normalize(&sql)))
        .collect();
        let rendered = triggers::render(&VAULT_MANIFEST);
        let expected = rendered
            .iter()
            .map(|trigger| (trigger.name.clone(), triggers::normalize(&trigger.sql)))
            .collect::<BTreeMap<_, _>>();
        if installed != expected {
            let sql = rendered
                .iter()
                .map(|trigger| format!("{};\n", trigger.sql))
                .collect::<Vec<_>>()
                .join("\n");
            panic!("installed sync triggers differ from the manifest; expected:\n\n{sql}");
        }
    });
}

/// A token of SQL text outside string literals and comments.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Symbol(char),
}

/// Splits SQL into lowercase words and symbols, dropping string literals and comments.
fn tokenize(sql: &str) -> Vec<Token> {
    let chars = sql.chars().collect::<Vec<_>>();
    let mut tokens = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        let current = chars[index];
        let next = chars.get(index + 1).copied();
        match current {
            c if c.is_whitespace() => index += 1,
            '-' if next == Some('-') => {
                while index < chars.len() && chars[index] != '\n' {
                    index += 1;
                }
            }
            '/' if next == Some('*') => {
                index += 2;
                while index + 1 < chars.len() && !(chars[index] == '*' && chars[index + 1] == '/') {
                    index += 1;
                }
                index += 2;
            }
            '\'' | '"' | '`' | '[' => {
                let close = if current == '[' { ']' } else { current };
                let mut text = String::new();
                index += 1;
                while index < chars.len() {
                    if chars[index] == close {
                        if close != ']' && chars.get(index + 1) == Some(&close) {
                            text.push(close);
                            index += 2;
                            continue;
                        }
                        break;
                    }
                    text.push(chars[index]);
                    index += 1;
                }
                index += 1;
                if current != '\'' {
                    tokens.push(Token::Word(text.to_lowercase()));
                }
            }
            c if c.is_alphanumeric() || c == '_' => {
                let start = index;
                while index < chars.len() && (chars[index].is_alphanumeric() || chars[index] == '_')
                {
                    index += 1;
                }
                let word = chars[start..index].iter().collect::<String>();
                tokens.push(Token::Word(word.to_lowercase()));
            }
            c => {
                tokens.push(Token::Symbol(c));
                index += 1;
            }
        }
    }
    tokens
}

/// The tokens inside every `CHECK (...)` of a `CREATE TABLE` statement.
fn check_expressions(sql: &str) -> Vec<Vec<Token>> {
    let tokens = tokenize(sql);
    let mut checks = Vec::new();
    let mut index = 0;
    while index < tokens.len() {
        let opens_check = tokens[index] == Token::Word("check".to_string())
            && tokens.get(index + 1) == Some(&Token::Symbol('('));
        if !opens_check {
            index += 1;
            continue;
        }
        let open = index + 1;
        let mut depth = 0_usize;
        let mut close = tokens.len();
        for (position, token) in tokens.iter().enumerate().skip(open) {
            match token {
                Token::Symbol('(') => depth += 1,
                Token::Symbol(')') => depth -= 1,
                _ => {}
            }
            if depth == 0 {
                close = position;
                break;
            }
        }
        checks.push(tokens[open + 1..close].to_vec());
        index = close;
    }
    checks
}

fn referenced_columns(expression: &[Token], columns: &BTreeSet<String>) -> BTreeSet<String> {
    expression
        .iter()
        .filter_map(|token| match token {
            Token::Word(word) if columns.contains(word) => Some(word.clone()),
            _ => None,
        })
        .collect()
}

/// Key columns of each non-primary unique index with their collations.
async fn unique_indexes(pool: &SqlitePool, table: &str) -> Vec<BTreeSet<(String, String)>> {
    let indexes: Vec<(String, String, i64)> = sqlx::query_as(
        "SELECT name, origin, partial FROM pragma_index_list(?) WHERE \"unique\" = 1",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .unwrap();
    let mut sets = Vec::new();
    for (name, origin, partial) in indexes {
        if origin == "pk" {
            continue;
        }
        assert_eq!(
            partial, 0,
            "{table}: partial unique index {name} has no resolution kind"
        );
        let columns: Vec<(Option<String>, String)> = sqlx::query_as(
            "SELECT name, coll FROM pragma_index_xinfo(?) WHERE key = 1 ORDER BY seqno",
        )
        .bind(&name)
        .fetch_all(pool)
        .await
        .unwrap();
        sets.push(
            columns
                .into_iter()
                .map(|(column, collation)| {
                    let column =
                        column.unwrap_or_else(|| panic!("{table}: expression index {name}"));
                    (column.to_lowercase(), collation.to_uppercase())
                })
                .collect(),
        );
    }
    sets
}

/// Declared unique column sets with the collation each column compares by.
fn declared_unique(table: &TableSpec) -> Vec<BTreeSet<(String, String)>> {
    table
        .resolutions
        .iter()
        .filter_map(|resolution| match resolution {
            Resolution::DuplicateRepair { columns, collation } => Some(
                columns
                    .iter()
                    .map(|column| {
                        let text = table
                            .groups
                            .iter()
                            .flat_map(|group| group.columns())
                            .find(|spec| spec.name == *column)
                            .is_some_and(|spec| spec.ty.is_text());
                        let collation = if text {
                            collation.sql_name()
                        } else {
                            Collation::Binary.sql_name()
                        };
                        (column.to_lowercase(), collation.to_string())
                    })
                    .collect(),
            ),
            Resolution::WithinGroup { .. } => None,
        })
        .collect()
}

fn declared_checks(table: &TableSpec) -> Vec<BTreeSet<String>> {
    table
        .resolutions
        .iter()
        .filter_map(|resolution| match resolution {
            Resolution::WithinGroup { columns, .. } => {
                Some(columns.iter().map(|column| column.to_lowercase()).collect())
            }
            Resolution::DuplicateRepair { .. } => None,
        })
        .collect()
}

#[test]
fn unique_and_multi_column_checks_have_resolutions() {
    block_on(async {
        let pool = migrated_pool().await;
        for table in VAULT_MANIFEST.tables {
            let mut unique = unique_indexes(&pool, table.name).await;
            unique.sort();
            let mut declared = declared_unique(table);
            declared.sort();
            assert_eq!(unique, declared, "{} unique resolutions", table.name);

            let sql: String = sqlx::query_scalar(
                "SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?",
            )
            .bind(table.name)
            .fetch_one(&pool)
            .await
            .unwrap();
            let columns = table_columns(&pool, table.name)
                .await
                .into_iter()
                .map(|(name, _, _)| name.to_lowercase())
                .collect::<BTreeSet<_>>();
            let mut multi_column = check_expressions(&sql)
                .iter()
                .map(|expression| referenced_columns(expression, &columns))
                .filter(|referenced| referenced.len() > 1)
                .collect::<Vec<_>>();
            multi_column.sort();
            let mut declared = declared_checks(table);
            declared.sort();
            assert_eq!(multi_column, declared, "{} check resolutions", table.name);
        }
    });
}

#[test]
fn check_scanner_ignores_strings_and_comments() {
    let sql = "CREATE TABLE t (a TEXT CHECK (a <> 'b AND c'), -- CHECK (b = c)\n\
               b INTEGER, \"c d\" TEXT, CHECK (b > 0 OR [c d] IS NULL) /* CHECK (a) */)";
    let columns = ["a", "b", "c d"].map(String::from).into_iter().collect();
    let referenced = check_expressions(sql)
        .iter()
        .map(|expression| referenced_columns(expression, &columns))
        .collect::<Vec<_>>();
    assert_eq!(
        referenced,
        [
            BTreeSet::from(["a".to_string()]),
            BTreeSet::from(["b".to_string(), "c d".to_string()]),
        ]
    );
}

fn assert_well_formed(manifest: &Manifest) {
    let mut table_ids = BTreeSet::new();
    for (position, table) in manifest.tables.iter().enumerate() {
        assert_ne!(table.id.0, 0, "{}: table id 0 is reserved", table.name);
        assert!(
            table_ids.insert(table.id),
            "{}: duplicate table id",
            table.name
        );
        assert!(!table.groups.is_empty(), "{}: no groups", table.name);

        let mut previous = None;
        for group in table.groups {
            assert!(
                previous.is_none_or(|id| id < group.id),
                "{}: groups out of id order",
                table.name
            );
            previous = Some(group.id);
            match group.storage {
                GroupStorage::Columns(columns) => {
                    assert!(
                        !columns.is_empty(),
                        "{}.{}: no columns",
                        table.name,
                        group.name
                    );
                }
                GroupStorage::Owned(_) => {
                    assert!(
                        table.adapter.is_some(),
                        "{}: owned storage needs an adapter",
                        table.name
                    );
                }
            }
            match group.kind {
                MergeKind::Immutable => {
                    assert!(
                        !group.surfaced && !group.recoverable && !group.bumps_revision,
                        "{}.{}: immutable groups never change",
                        table.name,
                        group.name
                    );
                    assert!(matches!(group.storage, GroupStorage::Columns(_)));
                }
                MergeKind::Coupled => {
                    assert!(
                        table.adapter.is_some(),
                        "{}: coupled groups need an adapter",
                        table.name
                    );
                }
                MergeKind::Reference { table: target } => {
                    let target_position = manifest
                        .tables
                        .iter()
                        .position(|candidate| candidate.id == target)
                        .unwrap_or_else(|| panic!("{}: unknown reference target", table.name));
                    assert!(
                        target_position < position,
                        "{}: parents come first",
                        table.name
                    );
                    assert_eq!(group.columns().len(), 1, "{}.{}", table.name, group.name);
                }
                MergeKind::Register | MergeKind::Max | MergeKind::Position => {}
            }
        }

        if let Some(revision) = table.revision_column {
            assert!(
                table.local_columns.contains(&revision),
                "{}: revision is local",
                table.name
            );
            assert!(table.groups.iter().any(|group| group.bumps_revision));
        } else {
            assert!(table.groups.iter().all(|group| !group.bumps_revision));
        }
        for resolution in table.resolutions {
            match resolution {
                Resolution::DuplicateRepair { columns, .. } => {
                    assert!(table.redirects, "{}: repairs redirect", table.name);
                    // Materialization moves colliding values aside through temporary text.
                    assert!(
                        table
                            .groups
                            .iter()
                            .flat_map(|group| group.columns())
                            .any(|spec| {
                                columns.contains(&spec.name) && spec.ty.placeholder(0).is_some()
                            }),
                        "{}: a repair rule needs a text column",
                        table.name
                    );
                    for column in *columns {
                        assert!(
                            table
                                .groups
                                .iter()
                                .flat_map(|group| group.columns())
                                .any(|spec| spec.name == *column),
                            "{}: repair column {column} is not replicated",
                            table.name
                        );
                    }
                }
                Resolution::WithinGroup { columns, group } => {
                    let group = table.group(*group).unwrap();
                    for column in *columns {
                        assert!(
                            group.columns().iter().any(|spec| spec.name == *column),
                            "{}: {column} is outside group {}",
                            table.name,
                            group.name
                        );
                    }
                }
            }
        }
    }
    for engine in manifest.engine {
        assert!(engine.starts_with("sync_"), "engine table {engine}");
    }
}

#[test]
fn vault_manifest_is_well_formed() {
    assert_well_formed(&VAULT_MANIFEST);
    assert_eq!(
        VAULT_MANIFEST
            .tables
            .iter()
            .map(|table| table.full_mask().0)
            .collect::<Vec<_>>(),
        [15, 255]
    );
}

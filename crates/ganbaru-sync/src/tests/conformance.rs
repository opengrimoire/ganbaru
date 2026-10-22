//! Manifests against their freshly migrated databases.

use std::collections::{BTreeMap, BTreeSet};

use sqlx::SqlitePool;

use super::fixture::{
    CYCLE_A, CYCLE_A_TABLE, CYCLE_B, CYCLE_B_TABLE, FIXTURE_DOMAIN, FIXTURE_TABLE_IDS, ITEMS,
    ITEMS_TABLE, LINKS, LINKS_TABLE, MEMBERSHIP_DETAILS, MEMBERSHIP_IDENTITY, MEMBERSHIPS,
    TEST_MANIFEST, fixture_pool, membership_group,
};
use super::{block_on, migrated_pool};
use crate::domains::vault_adapters;
use crate::manifest::order::{Dependency, DependencyKind};
use crate::manifest::vault::quick_notes;
use crate::manifest::vault::{VAULT_DOMAINS, VAULT_MANIFEST};
use crate::manifest::{
    Classification, Collation, ColumnSpec, ColumnType, DerivedKey, DomainManifest, GroupSpec,
    GroupStorage, LocalColumn, Manifest, MergeKind, OnDelete, ReferenceColumn, Resolution,
    SYNC_KEY_COLUMN, SYNC_TABLES, TableIdRange, TableSpec, is_sql_literal,
};
use crate::{Adapters, Engine, Hooks, SyncError, triggers};

/// Every manifest under test with a freshly migrated database that holds its schema.
pub(super) async fn cases() -> Vec<(&'static Manifest, SqlitePool)> {
    vec![
        (&VAULT_MANIFEST, migrated_pool().await),
        (&TEST_MANIFEST, fixture_pool().await),
    ]
}

pub(super) async fn schema_tables(pool: &SqlitePool) -> Vec<(String, String)> {
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
        for (manifest, pool) in cases().await {
            assert_classified(manifest, &pool).await;
        }
    });
}

async fn assert_classified(manifest: &Manifest, pool: &SqlitePool) {
    let tables = schema_tables(pool).await;
    let types = tables.iter().cloned().collect::<BTreeMap<_, _>>();

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

    for local in manifest.local {
        assert_eq!(
            types.get(*local).map(String::as_str),
            Some("table"),
            "local table {local} is missing"
        );
        assert_eq!(manifest.classify(local), Classification::Local);
    }
    for engine in manifest.engine {
        assert_eq!(
            types.get(*engine).map(String::as_str),
            Some("table"),
            "engine table {engine} is missing"
        );
        assert_eq!(manifest.classify(engine), Classification::Engine);
    }

    let sync_tables = tables
        .iter()
        .filter(|(name, _)| name.starts_with("sync_"))
        .map(|(name, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        sync_tables,
        SYNC_TABLES.iter().copied().collect::<BTreeSet<_>>(),
        "sync_* tables must match the engine tables every manifest declares"
    );
    for domain in manifest.domains {
        for table in domain.engine {
            assert!(
                !table.name.starts_with("sync_"),
                "{}: domain engine table {} uses the engine prefix",
                domain.name,
                table.name
            );
        }
    }

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
}

#[test]
fn replicated_columns_are_partitioned() {
    block_on(async {
        for (manifest, pool) in cases().await {
            assert_columns_partitioned(manifest, &pool).await;
        }
    });
}

async fn assert_columns_partitioned(manifest: &Manifest, pool: &SqlitePool) {
    let pool = pool.clone();
    for table in manifest.tables {
        let columns = table_columns(&pool, table.name).await;
        let mut declared = vec![table.key_column];
        declared.extend(
            table
                .groups
                .iter()
                .flat_map(|group| group.columns())
                .map(|column| column.name),
        );
        declared.extend(table.local_columns.iter().map(|column| column.name));
        declared.extend(table.derived_columns);
        let not_null: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) WHERE \"notnull\" = 1")
                .bind(table.name)
                .fetch_all(&pool)
                .await
                .unwrap();
        for column in table.local_columns {
            assert!(
                column.materialize != "NULL" || !not_null.iter().any(|name| name == column.name),
                "{}.{}: a NOT NULL local column needs a non-NULL materialization value",
                table.name,
                column.name
            );
        }
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
        if table.has_composite_key() {
            assert_composite_key(table, &columns, &not_null);
        } else {
            assert_eq!(
                primary_key,
                [(table.key_column, "TEXT")],
                "{} key",
                table.name
            );
        }

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
}

/// A composite primary key is keyed by a `sync_key TEXT NOT NULL UNIQUE` column outside it,
/// and its columns form one required `Immutable` identity group.
fn assert_composite_key(table: &TableSpec, columns: &[(String, String, i64)], not_null: &[String]) {
    let primary_key = columns
        .iter()
        .filter(|(_, _, pk)| *pk > 0)
        .map(|(name, _, _)| name.as_str())
        .collect::<BTreeSet<_>>();
    assert!(
        primary_key.len() > 1,
        "{}: a sync_key table has a composite primary key",
        table.name
    );
    assert!(
        columns
            .iter()
            .any(|(name, kind, pk)| name == SYNC_KEY_COLUMN && kind == "TEXT" && *pk == 0),
        "{}: sync_key is a text column outside the primary key",
        table.name
    );
    assert!(
        not_null.iter().any(|name| name == SYNC_KEY_COLUMN),
        "{}: sync_key is NOT NULL",
        table.name
    );
    let identity = table
        .groups
        .iter()
        .filter(|group| group.kind == MergeKind::Immutable)
        .find(|group| {
            group
                .columns()
                .iter()
                .any(|column| primary_key.contains(column.name))
        })
        .unwrap_or_else(|| panic!("{}: no identity group", table.name));
    assert_eq!(
        identity
            .columns()
            .iter()
            .map(|column| column.name)
            .collect::<BTreeSet<_>>(),
        primary_key,
        "{}: the identity group holds exactly the primary key",
        table.name
    );
}

/// Problems with the `Immutable` groups of a derived-key table: they hold only primary key
/// columns and declared key inputs, because adding a removed row again writes every group.
async fn derived_key_problems(table: &TableSpec, pool: &SqlitePool) -> Vec<String> {
    let Some(derived) = table.derived_key else {
        return Vec::new();
    };
    let primary_key = table_columns(pool, table.name)
        .await
        .into_iter()
        .filter(|(_, _, pk)| *pk > 0)
        .map(|(name, _, _)| name)
        .collect::<BTreeSet<_>>();
    table
        .immutable_columns()
        .filter(|column| {
            !primary_key.contains(column.name) && !derived.key_inputs.contains(&column.name)
        })
        .map(|column| {
            format!(
                "{}.{} is in an immutable group but is not a key column",
                table.name, column.name
            )
        })
        .collect()
}

#[test]
fn derived_key_immutable_groups_hold_only_key_columns() {
    block_on(async {
        for (manifest, pool) in cases().await {
            for table in manifest.tables {
                assert_eq!(
                    derived_key_problems(table, &pool).await,
                    Vec::<String>::new()
                );
            }
        }
    });
}

#[test]
fn derived_key_refuses_a_non_key_column_in_an_immutable_group() {
    const IDENTITY_WITH_TIME: GroupSpec = GroupSpec {
        storage: GroupStorage::Columns(&[
            ColumnSpec {
                name: "item_id",
                ty: ColumnType::OptionalRowKey,
            },
            ColumnSpec {
                name: "end_id",
                ty: ColumnType::OptionalRowKey,
            },
            ColumnSpec {
                name: "created_at",
                ty: ColumnType::Timestamp,
            },
        ]),
        ..MEMBERSHIP_IDENTITY
    };
    const ROLE_ONLY: GroupSpec = GroupSpec {
        storage: GroupStorage::Columns(&[ColumnSpec {
            name: "role",
            ty: ColumnType::Text { max_chars: 20 },
        }]),
        ..MEMBERSHIP_DETAILS
    };
    const TIME_IN_IDENTITY: TableSpec = TableSpec {
        groups: &[IDENTITY_WITH_TIME, ROLE_ONLY, MEMBERSHIPS.groups[2]],
        ..MEMBERSHIPS
    };
    block_on(async {
        let pool = fixture_pool().await;
        assert_eq!(
            derived_key_problems(&TIME_IN_IDENTITY, &pool).await,
            ["fixture_memberships.created_at is in an immutable group but is not a key column"]
        );
        // Declaring the column a key input admits it.
        const TIME_AS_INPUT: TableSpec = TableSpec {
            derived_key: Some(DerivedKey {
                exists_group: membership_group::EXISTS,
                key_inputs: &["created_at"],
            }),
            ..TIME_IN_IDENTITY
        };
        assert_eq!(
            derived_key_problems(&TIME_AS_INPUT, &pool).await,
            Vec::<String>::new()
        );
    });
}

#[test]
fn validation_refuses_malformed_derived_keys() {
    const IMMUTABLE_EXISTS: GroupSpec = GroupSpec {
        kind: MergeKind::Immutable,
        ..MEMBERSHIPS.groups[2]
    };
    const BAD_EXISTS: TableSpec = TableSpec {
        groups: &[MEMBERSHIP_IDENTITY, MEMBERSHIP_DETAILS, IMMUTABLE_EXISTS],
        redirects: true,
        derived_key: Some(DerivedKey {
            exists_group: membership_group::EXISTS,
            key_inputs: &["role"],
        }),
        ..MEMBERSHIPS
    };
    const NO_EXISTS: TableSpec = TableSpec {
        derived_key: None,
        ..MEMBERSHIPS
    };
    const BAD_DERIVED: DomainManifest = DomainManifest {
        tables: &[ITEMS, CYCLE_A, CYCLE_B, LINKS, BAD_EXISTS],
        ..FIXTURE_DOMAIN
    };
    const COLUMNLESS: DomainManifest = DomainManifest {
        tables: &[ITEMS, CYCLE_A, CYCLE_B, LINKS, NO_EXISTS],
        ..FIXTURE_DOMAIN
    };
    let bad = validation_error(&[&quick_notes::DOMAIN, &BAD_DERIVED]);
    for problem in [
        "existence group exists is not a hidden register without columns",
        "a derived-key table never redirects",
        "key input role is not an immutable column",
    ] {
        assert!(bad.contains(problem), "{problem}: {bad}");
    }
    let columnless = validation_error(&[&quick_notes::DOMAIN, &COLUMNLESS]);
    assert!(
        columnless.contains("exists has no columns and is not the existence group"),
        "{columnless}"
    );
}

#[test]
fn foreign_keys_follow_references_and_ownership() {
    block_on(async {
        for (manifest, pool) in cases().await {
            assert_foreign_keys(manifest, &pool).await;
        }
    });
}

/// Replicated rows are deleted by apply, so every foreign key into a replicated or owned child
/// table must clear or cascade instead of blocking the delete.
#[test]
fn foreign_keys_into_replicated_tables_cascade_or_clear() {
    block_on(async {
        for (manifest, pool) in cases().await {
            for (table, kind) in schema_tables(&pool).await {
                if kind != "table" {
                    continue;
                }
                for (column, target, _, on_delete) in foreign_keys(&pool, &table).await {
                    let converted = matches!(
                        manifest.classify(&target),
                        Classification::Replicated | Classification::OwnedChildren
                    );
                    assert!(
                        !converted || on_delete == "CASCADE" || on_delete == "SET NULL",
                        "{table}.{column} references {target} with ON DELETE {on_delete}"
                    );
                }
            }
        }
    });
}

async fn assert_foreign_keys(manifest: &Manifest, pool: &SqlitePool) {
    let pool = pool.clone();
    for table in manifest.tables {
        let mut expected = BTreeSet::new();
        for (_, _, reference) in table.references() {
            let target = manifest.table(reference.table).unwrap();
            expected.insert((
                reference.column.to_string(),
                target.name.to_string(),
                target.key_column.to_string(),
                reference.on_delete.sql_name().to_string(),
            ));
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
}

#[test]
fn installed_triggers_match_rendering() {
    block_on(async {
        for (manifest, pool) in cases().await {
            assert_triggers_rendered(manifest, &pool).await;
        }
    });
}

async fn assert_triggers_rendered(manifest: &Manifest, pool: &SqlitePool) {
    let pool = pool.clone();
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
    let rendered = triggers::render(manifest);
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
}

/// A token of SQL text outside string literals and comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Token {
    Word(String),
    Symbol(char),
}

/// Splits SQL into lowercase words and symbols, dropping string literals and comments.
pub(super) fn tokenize(sql: &str) -> Vec<Token> {
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
        for (manifest, pool) in cases().await {
            for table in manifest.tables {
                assert_resolutions(table, &pool).await;
            }
        }
    });
}

async fn assert_resolutions(table: &TableSpec, pool: &SqlitePool) {
    let pool = pool.clone();
    let mut unique = unique_indexes(&pool, table.name).await;
    unique.sort();
    let mut declared = declared_unique(table);
    // A sync_key is derived from the primary key, so it collides only when the key does.
    if table.has_composite_key() {
        declared.push(BTreeSet::from([(
            SYNC_KEY_COLUMN.to_string(),
            Collation::Binary.sql_name().to_string(),
        )]));
    }
    declared.sort();
    assert_eq!(unique, declared, "{} unique resolutions", table.name);

    let sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?")
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
    for table in manifest.tables {
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
                        !columns.is_empty() || table.is_exists_group(group.id),
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
                MergeKind::Reference { .. }
                | MergeKind::Register
                | MergeKind::Max
                | MergeKind::Position => {}
            }
        }

        if let Some(revision) = table.revision_column {
            assert!(
                table.local_column(revision).is_some(),
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
    for table in SYNC_TABLES {
        assert!(manifest.engine.contains(table), "engine table {table}");
    }
    manifest.validate().unwrap();
}

#[test]
fn manifests_are_well_formed() {
    block_on(async {
        for (manifest, _) in cases().await {
            assert_well_formed(manifest);
        }
    });
    assert_eq!(
        VAULT_MANIFEST
            .tables
            .iter()
            .map(|table| table.full_mask().0)
            .collect::<Vec<_>>(),
        [31, 511]
    );
}

#[test]
fn vault_domains_reserve_ids_below_the_fixture_range() {
    assert_eq!(VAULT_MANIFEST.domains, VAULT_DOMAINS);
    for domain in VAULT_DOMAINS {
        assert!(
            domain.table_ids.last < FIXTURE_TABLE_IDS,
            "{}: table ids from {FIXTURE_TABLE_IDS} are reserved for test fixtures",
            domain.name
        );
    }
}

fn leaked(domains: &'static [&'static DomainManifest]) -> &'static Manifest {
    Box::leak(Box::new(Manifest::from_domains(1, domains)))
}

fn validation_error(domains: &'static [&'static DomainManifest]) -> String {
    leaked(domains)
        .validate()
        .expect_err("the manifest should be refused")
}

#[test]
fn validation_refuses_overlapping_ranges_misplaced_ids_and_bad_literals() {
    const OVERLAPPING: DomainManifest = DomainManifest {
        name: "overlapping",
        table_ids: TableIdRange {
            first: 10,
            last: 20,
        },
        tables: &[],
        derived: &[],
        local: &[],
        engine: &[],
        replica_writable: false,
    };
    const MISPLACED: DomainManifest = DomainManifest {
        table_ids: TableIdRange {
            first: 100,
            last: 115,
        },
        ..quick_notes::DOMAIN
    };
    const EMPTY_RANGE: DomainManifest = DomainManifest {
        name: "empty_range",
        table_ids: TableIdRange {
            first: 200,
            last: 199,
        },
        ..OVERLAPPING
    };
    const BAD_NOTES: TableSpec = TableSpec {
        local_columns: &[LocalColumn {
            name: "revision",
            materialize: "revision + 1",
        }],
        ..quick_notes::NOTES
    };
    const BAD_LITERAL: DomainManifest = DomainManifest {
        tables: &[quick_notes::TAGS, BAD_NOTES],
        ..quick_notes::DOMAIN
    };

    let overlap = validation_error(&[&quick_notes::DOMAIN, &OVERLAPPING]);
    assert!(overlap.contains("ranges overlap"), "{overlap}");
    let twice = validation_error(&[&quick_notes::DOMAIN, &quick_notes::DOMAIN]);
    assert!(twice.contains("domain named twice"), "{twice}");
    assert!(twice.contains("duplicate table id"), "{twice}");
    let misplaced = validation_error(&[&MISPLACED]);
    assert!(
        misplaced.contains("outside the quick_notes range"),
        "{misplaced}"
    );
    let empty = validation_error(&[&EMPTY_RANGE]);
    assert!(empty.contains("invalid table id range"), "{empty}");
    let literal = validation_error(&[&BAD_LITERAL]);
    assert!(literal.contains("not a SQL literal"), "{literal}");
}

#[test]
fn validation_refuses_malformed_references() {
    const MALFORMED: [GroupSpec; 3] = {
        let mut groups = [LINKS.groups[0], LINKS.groups[1], LINKS.groups[2]];
        groups[0].references = &[ReferenceColumn {
            column: "owner_id",
            table: ITEMS_TABLE,
            on_delete: OnDelete::SetNull,
        }];
        groups[1].references = &[
            ReferenceColumn {
                column: "missing_id",
                table: CYCLE_A_TABLE,
                on_delete: OnDelete::SetNull,
            },
            ReferenceColumn {
                column: "note",
                table: CYCLE_A_TABLE,
                on_delete: OnDelete::SetNull,
            },
        ];
        groups[2].references = &[ReferenceColumn {
            column: "parent_id",
            table: LINKS_TABLE,
            on_delete: OnDelete::SetNull,
        }];
        groups
    };
    const BAD_LINKS: TableSpec = TableSpec {
        groups: &MALFORMED,
        ..LINKS
    };
    const BAD_REFERENCES: DomainManifest = DomainManifest {
        tables: &[ITEMS, CYCLE_A, CYCLE_B, BAD_LINKS],
        ..FIXTURE_DOMAIN
    };
    let malformed = validation_error(&[&BAD_REFERENCES]);
    for problem in [
        "owner_id is an immutable reference that does not cascade",
        "missing_id is not a column of the group",
        "note holds a reference but is not an optional row key",
        "a reference group declares no reference columns",
    ] {
        assert!(malformed.contains(problem), "{problem}: {malformed}");
    }
}

#[test]
fn validation_refuses_tables_out_of_dependency_order() {
    const SPLIT_CYCLE: DomainManifest = DomainManifest {
        tables: &[ITEMS, CYCLE_A, LINKS, CYCLE_B],
        ..FIXTURE_DOMAIN
    };
    let split = validation_error(&[&quick_notes::DOMAIN, &SPLIT_CYCLE]);
    assert!(
        split.contains("fixture_cycle_b: its dependency component is not contiguous"),
        "{split}"
    );
    assert!(
        split.contains("fixture_links: depends on fixture_cycle_b, which is listed later"),
        "{split}"
    );
    const TARGET_LAST: DomainManifest = DomainManifest {
        tables: &[CYCLE_A, CYCLE_B, LINKS, ITEMS],
        ..FIXTURE_DOMAIN
    };
    let late = validation_error(&[&quick_notes::DOMAIN, &TARGET_LAST]);
    assert!(
        late.contains("fixture_links: depends on fixture_items, which is listed later"),
        "{late}"
    );
    // A reference cycle in either order, and a reference to the row's own table, are allowed.
    const CYCLE_REVERSED: DomainManifest = DomainManifest {
        tables: &[ITEMS, CYCLE_B, CYCLE_A, LINKS],
        ..FIXTURE_DOMAIN
    };
    leaked(&[&quick_notes::DOMAIN, &CYCLE_REVERSED])
        .validate()
        .unwrap();
}

#[test]
fn owned_and_trigger_dependencies_never_lie_on_a_cycle() {
    let manifest = &*TEST_MANIFEST;
    let components = manifest.components(&manifest.reference_dependencies());
    let position = |id| {
        manifest
            .tables
            .iter()
            .position(|table| table.id == id)
            .unwrap()
    };
    assert_eq!(
        components[position(CYCLE_A_TABLE)],
        components[position(CYCLE_B_TABLE)]
    );
    assert_ne!(
        components[position(LINKS_TABLE)],
        components[position(CYCLE_A_TABLE)]
    );
    assert_eq!(components[position(LINKS_TABLE)], position(LINKS_TABLE));

    for kind in [DependencyKind::Owned, DependencyKind::ApplierVisible] {
        for (from, to) in [(CYCLE_A_TABLE, CYCLE_B_TABLE), (LINKS_TABLE, LINKS_TABLE)] {
            let mut dependencies = manifest.reference_dependencies();
            dependencies.push(Dependency { from, to, kind });
            let problems = manifest.order_problems(&dependencies);
            assert!(
                problems
                    .iter()
                    .any(|problem| problem.contains("lies on a cycle")),
                "{kind:?} {from:?} {to:?}: {problems:?}"
            );
        }
        let mut dependencies = manifest.reference_dependencies();
        dependencies.push(Dependency {
            from: LINKS_TABLE,
            to: CYCLE_B_TABLE,
            kind,
        });
        assert_eq!(manifest.order_problems(&dependencies), Vec::<String>::new());
    }
}

#[test]
fn local_materialization_values_are_single_literals() {
    for literal in ["0", "1", "-3", "NULL", "''", "'device'", "'it''s'"] {
        assert!(is_sql_literal(literal), "{literal}");
    }
    for expression in [
        "",
        "-",
        "null",
        "1 + 1",
        "now()",
        "'a' || 'b'",
        "'open",
        "'a'b'",
        "1;",
        "revision",
    ] {
        assert!(!is_sql_literal(expression), "{expression}");
    }
}

#[test]
fn engine_construction_checks_adapters_both_ways() {
    const TAGS_ONLY: DomainManifest = DomainManifest {
        tables: &[quick_notes::TAGS],
        derived: &[],
        ..quick_notes::DOMAIN
    };
    Engine::vault();
    let named = VAULT_MANIFEST
        .tables
        .iter()
        .filter_map(|table| table.adapter)
        .collect::<BTreeSet<_>>();
    assert_eq!(vault_adapters().names().collect::<BTreeSet<_>>(), named);

    let unregistered = Engine::with(&VAULT_MANIFEST, Adapters::new(), Hooks::default());
    assert!(
        matches!(&unregistered, Err(SyncError::InvalidManifest(detail)) if detail.contains("is not registered")),
        "{unregistered:?}"
    );
    let unnamed = Engine::with(leaked(&[&TAGS_ONLY]), vault_adapters(), Hooks::default());
    assert!(
        matches!(&unnamed, Err(SyncError::InvalidManifest(detail)) if detail.contains("no table names it")),
        "{unnamed:?}"
    );
    let invalid = Engine::with(
        leaked(&[&quick_notes::DOMAIN, &quick_notes::DOMAIN]),
        vault_adapters(),
        Hooks::default(),
    );
    assert!(
        matches!(&invalid, Err(SyncError::InvalidManifest(_))),
        "{invalid:?}"
    );
}

/// Recorded manifests by version. Operations of every version up to the current one validate
/// against the current manifest, so a recorded manifest never changes; a new version may only
/// add tables and groups or loosen rules, and it records its own snapshot here.
const RECORDED_MANIFESTS: &[(u16, &str)] = &[(1, include_str!("fixtures/vault_manifest_v1.txt"))];

#[test]
fn vault_manifest_changes_only_with_a_version_bump() {
    let (version, recorded) = RECORDED_MANIFESTS
        .last()
        .expect("at least one manifest is recorded");
    assert_eq!(
        *version, VAULT_MANIFEST.version,
        "record the snapshot of manifest version {}",
        VAULT_MANIFEST.version
    );
    assert_eq!(
        VAULT_MANIFEST.summary(),
        recorded.replace("\r\n", "\n"),
        "the vault manifest changed without a version bump"
    );
}

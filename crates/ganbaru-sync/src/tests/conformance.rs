//! Manifests against their freshly migrated databases.

use std::collections::{BTreeMap, BTreeSet};

use ganbaru_sync_contracts::bounds::{MAX_BLOB_FIELD_BYTES, MAX_TEXT_FIELD_BYTES};
use ganbaru_sync_contracts::{
    Change, ChangeAction, Field, GroupValue, OrderKey, RowKey, TableId, Value,
};
use sqlx::SqlitePool;

use super::fixture::{
    CYCLE_A, CYCLE_A_TABLE, CYCLE_B, CYCLE_B_TABLE, FIXTURE_DOMAIN, FIXTURE_TABLE_IDS, FixtureHook,
    ITEMS, ITEMS_TABLE, LINKS, LINKS_TABLE, LISTS_TABLE, MEMBERSHIP_DETAILS, MEMBERSHIP_IDENTITY,
    MEMBERSHIP_KEY_PREFIX, MEMBERSHIPS, MEMBERSHIPS_TABLE, NODES, TEST_MANIFEST, fixture_pool,
    membership_group, membership_key, node_group, test_engine,
};
use super::{block_on, migrated_pool};
use crate::assets::{AssetClass, MAX_ASSET_BYTES};
use crate::codec::{OwnedRow, decode_table, encode, encode_table};
use crate::domains::vault_adapters;
use crate::manifest::order::{Dependency, DependencyKind};
use crate::manifest::vault::quick_notes;
use crate::manifest::vault::{VAULT_DOMAINS, VAULT_MANIFEST};
use crate::manifest::{
    Classification, Collation, ColumnSpec, ColumnType, DerivedKey, DomainManifest, GroupSpec,
    GroupStorage, KeyDerivedColumn, LocalColumn, MAX_TIMESTAMP_BYTES, Manifest, MergeKind,
    OnDelete, OwnedBound, OwnedCodec, OwnedOrder, OwnedTable, ReferenceColumn, Resolution,
    SYNC_KEY_COLUMN, SYNC_LOCAL_TABLES, SYNC_TABLES, TableIdRange, TableSpec, TreeSpec, group_id,
    is_sql_literal,
};
use crate::sql::{read_owned, replace_owned};
use crate::{Adapters, Engine, Hooks, SyncError, triggers, validate};

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
        SYNC_TABLES
            .iter()
            .chain(SYNC_LOCAL_TABLES)
            .copied()
            .collect::<BTreeSet<_>>(),
        "sync_* tables must match the engine tables every manifest declares"
    );
    for table in SYNC_LOCAL_TABLES {
        assert_eq!(manifest.classify(table), Classification::Local, "{table}");
    }
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

/// Replicated columns never hold floating point values, whose text and binary forms do not
/// round-trip exactly through every replica.
fn assert_no_real(table: &str, columns: &[(String, String, i64)], replicated: &[&str]) {
    for (name, kind, _) in columns {
        let affinity = kind.to_uppercase();
        let real = ["REAL", "FLOA", "DOUB"]
            .iter()
            .any(|marker| affinity.contains(marker));
        assert!(
            !real || !replicated.contains(&name.as_str()),
            "{table}.{name}: a replicated column has floating point type {kind}"
        );
    }
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
        declared.extend(table.hook_columns);
        declared.extend(table.key_derived_columns.iter().map(|column| column.name));
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
        let defaults: Vec<(String, Option<String>)> =
            sqlx::query_as("SELECT name, dflt_value FROM pragma_table_info(?)")
                .bind(table.name)
                .fetch_all(&pool)
                .await
                .unwrap();
        for column in table.hook_columns {
            let nullable = !not_null.iter().any(|name| name == column);
            let defaulted = defaults
                .iter()
                .any(|(name, default)| name == column && default.is_some());
            assert!(
                nullable || defaulted,
                "{}.{column}: materialize inserts rows without hook columns, so they need a \
                 default or NULL",
                table.name
            );
        }
        let replicated = table
            .groups
            .iter()
            .flat_map(|group| group.columns())
            .map(|column| column.name)
            .collect::<Vec<_>>();
        assert_no_real(table.name, &columns, &replicated);
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
            let data = owned
                .columns
                .iter()
                .map(|column| column.name)
                .collect::<Vec<_>>();
            assert_no_real(owned.table, &child_columns, &data);
            let declared = owned.written_columns();
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
                std::iter::once(owned.parent_column)
                    .chain(owned.order.order_columns())
                    .collect::<BTreeSet<_>>(),
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
                prefix: Some(MEMBERSHIP_KEY_PREFIX),
                key_inputs: &["item_id", "end_id", "created_at"],
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
            prefix: Some("Not Valid"),
            key_inputs: &["role"],
        }),
        ..MEMBERSHIPS
    };
    const NO_EXISTS: TableSpec = TableSpec {
        derived_key: None,
        ..MEMBERSHIPS
    };
    const NO_PREFIX: TableSpec = TableSpec {
        derived_key: Some(DerivedKey {
            exists_group: membership_group::EXISTS,
            prefix: None,
            key_inputs: &["item_id", "end_id"],
        }),
        ..MEMBERSHIPS
    };
    const PREFIX_WITHOUT_INPUTS: TableSpec = TableSpec {
        derived_key: Some(DerivedKey {
            exists_group: membership_group::EXISTS,
            prefix: Some(MEMBERSHIP_KEY_PREFIX),
            key_inputs: &[],
        }),
        ..MEMBERSHIPS
    };
    const UNPREFIXED: DomainManifest = DomainManifest {
        tables: &[ITEMS, CYCLE_A, CYCLE_B, LINKS, NO_PREFIX],
        ..FIXTURE_DOMAIN
    };
    const INPUTLESS: DomainManifest = DomainManifest {
        tables: &[ITEMS, CYCLE_A, CYCLE_B, LINKS, PREFIX_WITHOUT_INPUTS],
        ..FIXTURE_DOMAIN
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
        "key prefix Not Valid is not a valid derived key prefix",
    ] {
        assert!(bad.contains(problem), "{problem}: {bad}");
    }
    let unprefixed = validation_error(&[&quick_notes::DOMAIN, &UNPREFIXED]);
    for problem in [
        "key inputs need a key prefix",
        "a composite-key table derives its key with a prefix",
    ] {
        assert!(unprefixed.contains(problem), "{problem}: {unprefixed}");
    }
    let inputless = validation_error(&[&quick_notes::DOMAIN, &INPUTLESS]);
    assert!(
        inputless.contains("a key prefix needs key inputs"),
        "{inputless}"
    );
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

/// A `published_with` column is nullable exactly when null publishes, and its foreign key
/// cascades: a nullable column that never publishes when null would keep such rows local
/// forever, and a key that clears would publish a parent's pending children at its delete.
#[test]
fn published_with_columns_are_nullable_only_when_null_publishes() {
    block_on(async {
        for (manifest, pool) in cases().await {
            for table in manifest.tables {
                let Some(with) = table.published_with else {
                    continue;
                };
                let not_null: Vec<String> = sqlx::query_scalar(
                    "SELECT name FROM pragma_table_info(?) WHERE \"notnull\" = 1",
                )
                .bind(table.name)
                .fetch_all(&pool)
                .await
                .unwrap();
                let nullable = !not_null.iter().any(|name| name == with.column);
                assert_eq!(
                    nullable, with.null_publishes,
                    "{}.{}: the column is nullable exactly when null publishes",
                    table.name, with.column
                );
                let cascades = foreign_keys(&pool, table.name).await.into_iter().any(
                    |(column, _, _, on_delete)| column == with.column && on_delete == "CASCADE",
                );
                assert!(
                    cascades,
                    "{}.{}: the foreign key into the publishing parent cascades",
                    table.name, with.column
                );
            }
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
        if let Some(parent) = table.key_parent {
            let parent = manifest.table(parent).unwrap();
            expected.insert((
                table.key_column.to_string(),
                parent.name.to_string(),
                parent.key_column.to_string(),
                OnDelete::Cascade.sql_name().to_string(),
            ));
        }
        let actual = foreign_keys(&pool, table.name)
            .await
            .into_iter()
            .collect::<BTreeSet<_>>();
        assert_eq!(actual, expected, "{} foreign keys", table.name);

        for group in table.groups {
            for owned in group.owned_tables() {
                assert_owned_foreign_keys(manifest, &pool, table, owned, None).await;
                for nested in owned.nested {
                    assert_owned_foreign_keys(manifest, &pool, table, nested, Some(owned)).await;
                }
            }
        }
    }
}

/// An owned table cascades from its owner row, or from its parent owned row through the parent
/// ordinal, and from each reference target. Only its own nested tables reference it.
async fn assert_owned_foreign_keys(
    manifest: &Manifest,
    pool: &SqlitePool,
    table: &TableSpec,
    owned: &OwnedTable,
    parent: Option<&OwnedTable>,
) {
    let cascade = |column: &str, target: &str, to: &str| {
        (
            column.to_string(),
            target.to_string(),
            to.to_string(),
            "CASCADE".to_string(),
        )
    };
    let mut expected = BTreeSet::new();
    match (parent, owned.order) {
        (Some(parent), OwnedOrder::Nested { parent_ordinal, .. }) => {
            let OwnedOrder::Ordinal(ordinal) = parent.order else {
                panic!("{}: the parent of a nested table is ordinal", owned.table);
            };
            expected.insert(cascade(
                owned.parent_column,
                parent.table,
                parent.parent_column,
            ));
            expected.insert(cascade(parent_ordinal, parent.table, ordinal));
        }
        (None, _) => {
            expected.insert(cascade(owned.parent_column, table.name, table.key_column));
        }
        (Some(_), _) => panic!("{}: a nested table orders by Nested", owned.table),
    }
    for reference in owned.references {
        let target = manifest.table(reference.table).unwrap();
        expected.insert(cascade(reference.column, target.name, target.key_column));
    }
    // A foreign key between rows of one owned table is replaced with them.
    let actual = foreign_keys(pool, owned.table)
        .await
        .into_iter()
        .filter(|(_, target, _, _)| target != owned.table)
        .collect::<BTreeSet<_>>();
    assert_eq!(actual, expected, "{} foreign keys", owned.table);

    for (other, kind) in schema_tables(pool).await {
        if kind != "table" || other == owned.table {
            continue;
        }
        let nested_here = owned.nested.iter().any(|nested| nested.table == other);
        let referenced = foreign_keys(pool, &other)
            .await
            .into_iter()
            .any(|(_, target, _, _)| target == owned.table);
        assert!(
            nested_here || !referenced,
            "{other} references owned table {}",
            owned.table
        );
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
                for owned in owned_tables(table) {
                    assert_owned_constraints(owned, &pool).await;
                }
            }
        }
    });
}

/// Every owned table of `table`, nested ones included.
fn owned_tables(table: &TableSpec) -> Vec<&'static OwnedTable> {
    let mut pending = table
        .groups
        .iter()
        .flat_map(|group| group.owned_tables())
        .collect::<Vec<_>>();
    let mut all = Vec::new();
    while let Some(owned) = pending.pop() {
        pending.extend(owned.nested);
        all.push(owned);
    }
    all
}

/// An owned table's unique indexes are its declared unique id columns, compared exactly, and
/// none of its checks spans several columns, because materialize writes a whole blob at once
/// and has no repair for owned rows.
async fn assert_owned_constraints(owned: &OwnedTable, pool: &SqlitePool) {
    let mut unique = unique_indexes(pool, owned.table).await;
    unique.sort();
    let mut declared = owned
        .unique
        .iter()
        .map(|column| {
            BTreeSet::from([(
                column.to_lowercase(),
                Collation::Binary.sql_name().to_string(),
            )])
        })
        .collect::<Vec<_>>();
    declared.sort();
    assert_eq!(unique, declared, "{} unique columns", owned.table);

    let sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_schema WHERE type = 'table' AND name = ?")
            .bind(owned.table)
            .fetch_one(pool)
            .await
            .unwrap();
    let columns = table_columns(pool, owned.table)
        .await
        .into_iter()
        .map(|(name, _, _)| name.to_lowercase())
        .collect::<BTreeSet<_>>();
    let multi_column = check_expressions(&sql)
        .iter()
        .map(|expression| referenced_columns(expression, &columns))
        .filter(|referenced| referenced.len() > 1)
        .collect::<Vec<_>>();
    assert_eq!(
        multi_column,
        Vec::<BTreeSet<String>>::new(),
        "{} multi-column checks",
        owned.table
    );
}

/// The unique column set a derived key implies: the row key is a function of its non-empty key
/// inputs, so two rows with equal inputs are one row and never need duplicate repair. A
/// composite-key table derives its key from its primary key, which is unique already.
fn implied_unique(table: &TableSpec) -> Option<BTreeSet<(String, String)>> {
    let derived = table.derived_key?;
    (!derived.key_inputs.is_empty() && !table.has_composite_key()).then(|| {
        derived
            .key_inputs
            .iter()
            .map(|column| {
                (
                    column.to_lowercase(),
                    Collation::Binary.sql_name().to_string(),
                )
            })
            .collect()
    })
}

/// Columns of a unique index, each with its collation.
type UniqueSet = BTreeSet<(String, String)>;

/// The unique column sets and multi-column CHECKs key-derived columns imply: a column holding
/// a prefix followed by the row key collides only when the key does, and its CHECK reads the
/// column and the key.
fn key_derived_implications(table: &TableSpec) -> (Vec<UniqueSet>, Vec<BTreeSet<String>>) {
    table
        .key_derived_columns
        .iter()
        .map(|column| {
            let name = column.name.to_lowercase();
            (
                BTreeSet::from([(name.clone(), Collation::Binary.sql_name().to_string())]),
                BTreeSet::from([name, table.key_column.to_lowercase()]),
            )
        })
        .unzip()
}

/// Whether a unique set holds the row key of a table keyed by one column. The key is unique on
/// its own, so a set holding it collides only when the key does, and an index on it exists only
/// for foreign keys that target the set.
fn contains_key(table: &TableSpec, columns: &UniqueSet) -> bool {
    !table.has_composite_key()
        && columns.len() > 1
        && columns
            .iter()
            .any(|(column, _)| *column == table.key_column.to_lowercase())
}

/// The multi-column CHECK a tree implies: a parent that differs from the row's key. A row naming
/// itself is a cycle of length 1, which tree repair breaks before the row materializes.
fn tree_implication(table: &TableSpec) -> Option<BTreeSet<String>> {
    let tree = table.tree?;
    let [parent] = table.group(tree.parent_group)?.columns() else {
        return None;
    };
    Some(BTreeSet::from([
        parent.name.to_lowercase(),
        table.key_column.to_lowercase(),
    ]))
}

async fn assert_resolutions(table: &TableSpec, pool: &SqlitePool) {
    let pool = pool.clone();
    let implied = implied_unique(table);
    let (key_unique, key_checks) = key_derived_implications(table);
    let mut unique = unique_indexes(&pool, table.name)
        .await
        .into_iter()
        .filter(|columns| {
            implied.as_ref() != Some(columns)
                && !key_unique.contains(columns)
                && !contains_key(table, columns)
        })
        .collect::<Vec<_>>();
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
    // Every key-derived column is pinned to the key, so no local write can change it.
    for check in &key_checks {
        let position = multi_column.iter().position(|columns| columns == check);
        let position = position.unwrap_or_else(|| {
            panic!(
                "{}: key-derived column {check:?} needs a CHECK against the key",
                table.name
            )
        });
        multi_column.remove(position);
    }
    if let Some(implied) = tree_implication(table) {
        multi_column.retain(|columns| *columns != implied);
    }
    multi_column.sort();
    let mut declared = declared_checks(table);
    declared.sort();
    assert_eq!(multi_column, declared, "{} check resolutions", table.name);
}

/// Every owned table codec round-trips through its schema: rows encoded, written by
/// materialize, read back by capture, and encoded again give the same bytes, so a replica never
/// seals a value that differs from the one it applied.
#[test]
fn owned_codecs_round_trip_through_the_schema() {
    block_on(async {
        for ((manifest, pool), engine) in cases()
            .await
            .into_iter()
            .zip([Engine::vault(), test_engine()])
        {
            for table in manifest.tables {
                for owned in table.groups.iter().flat_map(|group| group.owned_tables()) {
                    let required = required_columns(&pool, owned).await;
                    for rows in owned_samples(&engine, table, owned, &required) {
                        assert_owned_round_trip(&engine, &pool, table, owned, rows).await;
                    }
                }
            }
        }
    });
}

/// `(table, column)` of every `NOT NULL` column of an owned table and its nested tables.
type RequiredColumns = BTreeSet<(&'static str, String)>;

/// The `NOT NULL` columns of `owned` and its nested tables, which samples never leave null:
/// the adapter refuses a null there before it reaches the schema.
async fn required_columns(pool: &SqlitePool, owned: &OwnedTable) -> RequiredColumns {
    let mut required = RequiredColumns::new();
    let mut pending = vec![owned];
    while let Some(table) = pending.pop() {
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info(?) WHERE \"notnull\" = 1")
                .bind(table.table)
                .fetch_all(pool)
                .await
                .unwrap();
        required.extend(columns.into_iter().map(|column| (table.table, column)));
        pending.extend(table.nested);
    }
    required
}

/// Row lists that reach an owned table's bounds: generated for the generic codec, from the
/// domain adapter for a custom one.
fn owned_samples(
    engine: &Engine,
    table: &TableSpec,
    owned: &OwnedTable,
    required: &RequiredColumns,
) -> Vec<Vec<OwnedRow>> {
    match owned.codec {
        OwnedCodec::Generic => {
            let generate = |max_text_chars: usize| -> Vec<Vec<OwnedRow>> {
                let mut next = 0;
                [0, 1, owned.max_rows]
                    .into_iter()
                    .map(|count| generated_rows(owned, count, &mut next, required, max_text_chars))
                    .collect()
            };
            if owned.bound == OwnedBound::Static {
                return generate(usize::MAX);
            }
            // A domain-bounded table reaches its row limits with the longest texts whose
            // encoding still fits one blob field, as the domain's write validation keeps it.
            let mut max_text_chars = MAX_TEXT_FIELD_BYTES;
            loop {
                let samples = generate(max_text_chars);
                let fits = samples.iter().all(|rows| {
                    encode(owned, rows).is_ok_and(|bytes| bytes.len() <= MAX_BLOB_FIELD_BYTES)
                });
                if fits {
                    return samples;
                }
                assert!(max_text_chars > 1, "{}: no sample fits", owned.table);
                max_text_chars /= 2;
            }
        }
        OwnedCodec::Custom => {
            let adapter = engine
                .adapter(table)
                .expect("a custom codec has an adapter");
            let samples = adapter.owned_samples(table.id, owned.table);
            assert!(!samples.is_empty(), "{}: no codec samples", owned.table);
            samples
                .into_iter()
                .map(|rows| rows.into_iter().map(OwnedRow::flat).collect())
                .collect()
        }
    }
}

/// `count` rows with fields at their column bounds, texts at most `max_text_chars` long, unique
/// values numbered from `next`, and nested tables filled the same way.
fn generated_rows(
    owned: &OwnedTable,
    count: usize,
    next: &mut usize,
    required: &RequiredColumns,
    max_text_chars: usize,
) -> Vec<OwnedRow> {
    (0..count)
        .map(|position| {
            let serial = *next;
            *next += 1;
            let fields = owned
                .columns
                .iter()
                .map(|column| {
                    let unique = owned.unique.contains(&column.name);
                    let nullable = !required.contains(&(owned.table, column.name.to_string()));
                    generated_field(column.ty, serial, unique, nullable, max_text_chars)
                })
                .collect();
            let nested = owned
                .nested
                .iter()
                .map(|nested| {
                    generated_rows(
                        nested,
                        (position % 2) * nested.max_rows,
                        next,
                        required,
                        max_text_chars,
                    )
                })
                .collect();
            OwnedRow { fields, nested }
        })
        .collect()
}

/// A field of `ty` at its upper bound for even serials and its lower bound for odd ones,
/// starting with the serial when it must be unique, and null at the lower bound only when
/// `nullable`.
fn generated_field(
    ty: ColumnType,
    serial: usize,
    unique: bool,
    nullable: bool,
    max_text_chars: usize,
) -> Field {
    let low = serial % 2 == 1 && !unique;
    let text = |min_chars: usize, max_chars: usize, fill: char| {
        let prefix = if unique {
            serial.to_string()
        } else {
            String::new()
        };
        let length = if low {
            min_chars
        } else {
            max_chars.min(max_text_chars)
        };
        let fill_chars = length.saturating_sub(prefix.chars().count());
        Field::Text(format!("{prefix}{}", fill.to_string().repeat(fill_chars)))
    };
    match ty {
        ColumnType::Text { max_chars } => text(0, max_chars, 'é'),
        ColumnType::OptionalText { .. } if low && nullable => Field::Null,
        ColumnType::OptionalText { max_chars } => text(0, max_chars, 'é'),
        ColumnType::TrimmedText {
            min_chars,
            max_chars,
        } => text(min_chars.max(1), max_chars, 'w'),
        ColumnType::Integer { min, max } => Field::Integer(if low { min } else { max }),
        ColumnType::OptionalInteger { .. } if low && nullable && serial % 4 == 1 => Field::Null,
        ColumnType::OptionalInteger { min, max } => Field::Integer(if low { min } else { max }),
        ColumnType::Flag => Field::Integer(i64::from(!low)),
        ColumnType::Timestamp => Field::Text("9".repeat(MAX_TIMESTAMP_BYTES)),
        ColumnType::OptionalTimestamp if low && nullable => Field::Null,
        ColumnType::OptionalTimestamp => Field::Text("9".repeat(MAX_TIMESTAMP_BYTES)),
        ColumnType::OrderKey => Field::Text(
            OrderKey::rank(u32::try_from(serial).unwrap())
                .unwrap()
                .into_string(),
        ),
        ColumnType::OptionalRowKey if low && nullable => Field::Null,
        ColumnType::OptionalRowKey => Field::Text(format!("key-{serial}")),
    }
}

/// Writes `rows` under a placeholder owner inside a rolled back transaction whose foreign keys
/// are deferred, reads them back, and compares the encodings.
async fn assert_owned_round_trip(
    engine: &Engine,
    pool: &SqlitePool,
    table: &TableSpec,
    owned: &OwnedTable,
    rows: Vec<OwnedRow>,
) {
    const OWNER: &str = "round-trip-owner";
    let encoded = encode_table(engine, table, owned, rows.clone())
        .unwrap_or_else(|error| panic!("{}: sample refused: {error}", owned.table));
    assert_eq!(
        decode_table(engine, table, owned, &encoded).unwrap(),
        rows,
        "{} decode",
        owned.table
    );
    let mut transaction = pool.begin().await.unwrap();
    sqlx::raw_sql(
        "PRAGMA defer_foreign_keys = ON;
         UPDATE sync_apply_state SET applying = 1;",
    )
    .execute(&mut *transaction)
    .await
    .unwrap();
    replace_owned(&mut transaction, owned, OWNER, &rows)
        .await
        .unwrap_or_else(|error| panic!("{}: schema refused a sample: {error}", owned.table));
    let read = read_owned(&mut transaction, owned, OWNER)
        .await
        .unwrap()
        .unwrap_or_else(|| panic!("{}: rows did not read back", owned.table));
    transaction.rollback().await.unwrap();
    assert_eq!(
        encode_table(engine, table, owned, read).unwrap(),
        encoded,
        "{} round trip",
        owned.table
    );
}

#[test]
fn a_unique_set_of_derived_key_inputs_needs_no_repair() {
    const KEYED_BY_INPUTS: TableSpec = TableSpec {
        name: "fixture_keyed_pairs",
        key_column: "id",
        derived_key: Some(DerivedKey {
            exists_group: membership_group::EXISTS,
            prefix: Some(MEMBERSHIP_KEY_PREFIX),
            key_inputs: &["item_id", "end_id"],
        }),
        ..MEMBERSHIPS
    };
    const ONE_INPUT: TableSpec = TableSpec {
        derived_key: Some(DerivedKey {
            exists_group: membership_group::EXISTS,
            prefix: Some(MEMBERSHIP_KEY_PREFIX),
            key_inputs: &["item_id"],
        }),
        ..KEYED_BY_INPUTS
    };
    block_on(async {
        let pool = fixture_pool().await;
        sqlx::raw_sql(
            "CREATE TABLE fixture_keyed_pairs (
                 id TEXT PRIMARY KEY,
                 item_id TEXT NOT NULL,
                 end_id TEXT NOT NULL,
                 created_at TEXT NOT NULL,
                 role TEXT NOT NULL CHECK (length(role) <= 20),
                 UNIQUE (item_id, end_id)
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        assert_resolutions(&KEYED_BY_INPUTS, &pool).await;

        let unique = unique_indexes(&pool, KEYED_BY_INPUTS.name).await;
        assert_eq!(unique.first(), implied_unique(&KEYED_BY_INPUTS).as_ref());
        // A unique set that differs from the key inputs still needs a resolution.
        assert_ne!(unique.first(), implied_unique(&ONE_INPUT).as_ref());
        assert_eq!(implied_unique(&MEMBERSHIPS), None);
    });
}

#[test]
fn a_row_keyed_by_its_parent_cascades_and_unique_sets_holding_the_key_need_no_repair() {
    const ITEM_NOTES: TableSpec = TableSpec {
        id: TableId(FIXTURE_TABLE_IDS + 15),
        name: "fixture_item_notes",
        key_column: "item_id",
        groups: &[GroupSpec {
            id: group_id(0),
            name: "note",
            kind: MergeKind::Register,
            storage: GroupStorage::Columns(&[ColumnSpec {
                name: "note",
                ty: ColumnType::Text { max_chars: 40 },
            }]),
            references: &[],
            surfaced: false,
            recoverable: false,
            bumps_revision: false,
        }],
        local_columns: &[],
        triggers: &[],
        key_parent: Some(ITEMS_TABLE),
        ..ITEMS
    };
    const PARENT_FIRST: DomainManifest = DomainManifest {
        tables: &[ITEMS, ITEM_NOTES],
        ..FIXTURE_DOMAIN
    };
    const CHILD_FIRST: DomainManifest = DomainManifest {
        tables: &[ITEM_NOTES, ITEMS],
        ..FIXTURE_DOMAIN
    };
    block_on(async {
        let pool = fixture_pool().await;
        sqlx::raw_sql(
            "CREATE TABLE fixture_item_notes (
                 item_id TEXT PRIMARY KEY REFERENCES fixture_items(id) ON DELETE CASCADE,
                 note TEXT NOT NULL CHECK (length(note) <= 40),
                 UNIQUE (item_id, note)
             )",
        )
        .execute(&pool)
        .await
        .unwrap();
        let parent_first = leaked(&[&PARENT_FIRST]);
        parent_first.validate().unwrap();
        assert_foreign_keys(parent_first, &pool).await;
        assert_resolutions(&ITEM_NOTES, &pool).await;
        // The parent must materialize first.
        assert!(validation_error(&[&CHILD_FIRST]).contains("fixture_item_notes"));

        let binary = || Collation::Binary.sql_name().to_string();
        let with_key = BTreeSet::from([
            ("item_id".to_string(), binary()),
            ("note".to_string(), binary()),
        ]);
        assert!(contains_key(&ITEM_NOTES, &with_key));
        // A unique set without the key can collide between distinct rows.
        assert!(!contains_key(
            &ITEM_NOTES,
            &BTreeSet::from([("note".to_string(), binary())])
        ));
        assert!(!contains_key(&MEMBERSHIPS, &with_key));
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
                GroupStorage::Owned { tables, .. } => {
                    assert!(
                        table.adapter.is_some()
                            || tables
                                .iter()
                                .all(|owned| owned.codec == OwnedCodec::Generic),
                        "{}: a custom owned codec needs an adapter",
                        table.name
                    );
                }
            }
            match group.kind {
                MergeKind::Immutable => {
                    assert!(
                        !group.surfaced && !group.bumps_revision,
                        "{}.{}: immutable groups never change",
                        table.name,
                        group.name
                    );
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
        [
            31, 511, 7, 127, 31, 7, 3, 7, 1023, 7, 127, 31, 63, 15, 63, 3, 2047, 15, 31, 31, 15,
            255, 127, 65535, 31, 7, 31, 255, 127, 31, 8191, 31, 15, 7, 31, 15, 15, 1, 15, 15, 31,
            31, 15, 63, 134217727, 65535, 3, 15, 63, 15, 7, 1, 63, 3, 7, 31, 31, 31, 7, 15, 1, 15,
            1023, 127, 255, 255, 31, 7, 31, 127, 7, 7, 63, 31, 15, 15, 31, 7, 7, 3, 15, 31, 1, 31,
            1, 15, 31, 31, 15, 15, 31, 1, 15, 3, 1, 1, 1, 1, 1, 1, 15, 15, 15, 15, 31, 1
        ]
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
        unconverted: &[],
        assets: &[],
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
fn validation_refuses_key_derived_columns_naming_the_key_or_on_composite_keys() {
    const KEY_NAMED: TableSpec = TableSpec {
        key_derived_columns: &[KeyDerivedColumn {
            name: "id",
            prefix: "tag-",
        }],
        ..quick_notes::TAGS
    };
    const COMPOSITE: TableSpec = TableSpec {
        key_column: SYNC_KEY_COLUMN,
        key_derived_columns: &[KeyDerivedColumn {
            name: "handle",
            prefix: "note-",
        }],
        ..quick_notes::NOTES
    };
    const BAD_KEY_DERIVED: DomainManifest = DomainManifest {
        tables: &[KEY_NAMED, COMPOSITE],
        ..quick_notes::DOMAIN
    };
    let problems = validation_error(&[&BAD_KEY_DERIVED]);
    for problem in [
        "quick_note_tags: a key-derived column needs its own name",
        "quick_notes.handle: a composite-key table has no key-derived columns",
    ] {
        assert!(problems.contains(problem), "{problems}");
    }
}

#[test]
fn a_tree_orders_siblings_by_a_position_group_or_by_none() {
    const UNORDERED: TableSpec = TableSpec {
        tree: Some(TreeSpec {
            position_group: None,
            ..NODES_TREE
        }),
        ..NODES
    };
    const MISORDERED: TableSpec = TableSpec {
        tree: Some(TreeSpec {
            position_group: Some(node_group::LABEL),
            ..NODES_TREE
        }),
        ..NODES
    };
    const UNORDERED_DOMAIN: DomainManifest = DomainManifest {
        tables: &[UNORDERED],
        ..FIXTURE_DOMAIN
    };
    const MISORDERED_DOMAIN: DomainManifest = DomainManifest {
        tables: &[MISORDERED],
        ..FIXTURE_DOMAIN
    };
    const NODES_TREE: TreeSpec = match NODES.tree {
        Some(tree) => tree,
        None => panic!("the node fixture is a tree"),
    };

    leaked(&[&UNORDERED_DOMAIN]).validate().unwrap();
    let misordered = validation_error(&[&MISORDERED_DOMAIN]);
    assert!(
        misordered.contains("the tree position is not a position group"),
        "{misordered}"
    );

    let summary = leaked(&[&UNORDERED_DOMAIN]).summary();
    assert!(
        summary.contains("tree parent 0 position none depth 2"),
        "{summary}"
    );
    let summary = leaked(&[&FIXTURE_DOMAIN]).summary();
    assert!(
        summary.contains("tree parent 0 position 1 depth 2"),
        "{summary}"
    );

    // The node schema refuses a self parent, a CHECK over the parent and the key that the tree
    // implies, so it needs no declared resolution.
    assert_eq!(
        tree_implication(&NODES),
        Some(BTreeSet::from(["parent_id".to_string(), "id".to_string()]))
    );
    assert_eq!(tree_implication(&quick_notes::NOTES), None);
}

#[test]
fn validation_refuses_repeated_unsafe_and_oversized_asset_classes() {
    const REPEATED: DomainManifest = DomainManifest {
        assets: &[quick_notes::IMAGES_CLASS],
        ..FIXTURE_DOMAIN
    };
    let repeated = validation_error(&[&quick_notes::DOMAIN, &REPEATED]);
    assert!(
        repeated.contains("quick-notes/images: asset class named twice"),
        "{repeated}"
    );
    const UNSAFE: DomainManifest = DomainManifest {
        assets: &[
            AssetClass {
                name: "../outside",
                max_bytes: 1,
            },
            AssetClass {
                name: "huge",
                max_bytes: MAX_ASSET_BYTES + 1,
            },
        ],
        ..FIXTURE_DOMAIN
    };
    let unsafe_classes = validation_error(&[&UNSAFE]);
    for problem in [
        "asset class ../outside: class name is not a relative path",
        "asset class huge: class byte bound is outside",
    ] {
        assert!(unsafe_classes.contains(problem), "{unsafe_classes}");
    }
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
    let unknown_hook = Engine::with(
        &VAULT_MANIFEST,
        vault_adapters(),
        Hooks::new().with(LISTS_TABLE, Box::new(FixtureHook)),
    );
    assert!(
        matches!(&unknown_hook, Err(SyncError::InvalidManifest(detail)) if detail.contains("unknown table")),
        "{unknown_hook:?}"
    );
}

/// Recorded manifests by version. Operations of every version up to the current one validate
/// against the current manifest, so a recorded manifest never changes; a new version may only
/// add tables and groups or loosen rules, and it records its own snapshot here.
const RECORDED_MANIFESTS: &[(u16, &str)] = &[
    (1, include_str!("fixtures/vault_manifest_v1.txt")),
    (2, include_str!("fixtures/vault_manifest_v2.txt")),
    (3, include_str!("fixtures/vault_manifest_v3.txt")),
    (4, include_str!("fixtures/vault_manifest_v4.txt")),
    (5, include_str!("fixtures/vault_manifest_v5.txt")),
    (6, include_str!("fixtures/vault_manifest_v6.txt")),
    (7, include_str!("fixtures/vault_manifest_v7.txt")),
    (8, include_str!("fixtures/vault_manifest_v8.txt")),
    (9, include_str!("fixtures/vault_manifest_v9.txt")),
    (10, include_str!("fixtures/vault_manifest_v10.txt")),
    (11, include_str!("fixtures/vault_manifest_v11.txt")),
    (12, include_str!("fixtures/vault_manifest_v12.txt")),
    (13, include_str!("fixtures/vault_manifest_v13.txt")),
    (14, include_str!("fixtures/vault_manifest_v14.txt")),
    (15, include_str!("fixtures/vault_manifest_v15.txt")),
    (16, include_str!("fixtures/vault_manifest_v16.txt")),
    (17, include_str!("fixtures/vault_manifest_v17.txt")),
    (18, include_str!("fixtures/vault_manifest_v18.txt")),
    (19, include_str!("fixtures/vault_manifest_v19.txt")),
    (20, include_str!("fixtures/vault_manifest_v20.txt")),
    (21, include_str!("fixtures/vault_manifest_v21.txt")),
    (22, include_str!("fixtures/vault_manifest_v22.txt")),
];
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

/// A full membership value under `row`, whose identity names `item` in `end`.
fn membership_change(row: &str, action: ChangeAction, item: &str, end: &str) -> Change {
    let text = |value: &str| Field::Text(value.to_string());
    let group = |group, fields| {
        GroupValue::new(
            MEMBERSHIPS_TABLE,
            group,
            Value::new(fields).expect("fixture values are in bounds"),
        )
    };
    Change {
        table: MEMBERSHIPS_TABLE,
        row: RowKey::new(row).expect("fixture keys are valid"),
        action,
        groups: vec![
            group(membership_group::IDENTITY, vec![text(item), text(end)]),
            group(
                membership_group::DETAILS,
                vec![text("2026-01-01T00:00:00Z"), text("member")],
            ),
            group(membership_group::EXISTS, vec![Field::Integer(1)]),
        ],
        replaced_by: None,
        cause: None,
    }
}

#[test]
fn a_derived_key_row_is_valid_only_under_the_key_its_inputs_derive() {
    let engine = test_engine();
    let derived = membership_key("i-1", "a-1");
    let other = membership_key("i-2", "a-1");
    for action in [ChangeAction::Create, ChangeAction::Write] {
        let change = membership_change(&derived, action, "i-1", "a-1");
        assert_eq!(validate::change(&engine, &change), Ok(()));
        for row in [other.as_str(), "m-1"] {
            let forged = membership_change(row, action, "i-1", "a-1");
            assert_eq!(
                validate::change(&engine, &forged),
                Err(validate::DERIVED_KEY_MISMATCH)
            );
        }
    }
}

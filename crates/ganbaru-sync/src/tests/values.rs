//! Value guard triggers and their agreement with the contract types.

use ganbaru_sync_contracts::bounds::{MAX_ORDER_KEY_BYTES, MAX_ROW_KEY_BYTES};
use ganbaru_sync_contracts::{OrderKey, RowKey};
use sqlx::SqlitePool;

use super::{block_on, execute, migrated_pool, rejects, set_applying};
use crate::manifest::vault::VAULT_MANIFEST;
use crate::manifest::{ColumnType, MAX_TIMESTAMP_BYTES};
use crate::triggers::{row_key_rule, value_rule};

async fn rule_accepts(pool: &SqlitePool, rule: &str, value: &str) -> bool {
    let accepted: Option<i64> = sqlx::query_scalar(&format!("SELECT ({rule}) IS 1"))
        .bind(value)
        .fetch_one(pool)
        .await
        .unwrap();
    accepted == Some(1)
}

/// A small deterministic generator, so failures reproduce.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn below(&mut self, bound: usize) -> usize {
        usize::try_from(self.next()).unwrap() % bound
    }
}

fn order_key_corpus() -> Vec<String> {
    let mut corpus = [
        "",
        "a",
        "a0",
        "a1",
        "a00",
        "a01",
        "a0V",
        "a0V0",
        "b00",
        "b001",
        "b0z",
        "Zz",
        "Z0",
        "Z00",
        "zzzzzzzzzzzzzzzzzzzzzzzzzzz",
        "zzzzzzzzzzzzzzzzzzzzzzzzzz",
        "A00000000000000000000000000",
        "A000000000000000000000000001",
        "A000000000000000000000000000",
        "c000",
        "c00",
        "a0 ",
        " a0",
        "a-0",
        "é0",
        "a\u{0}1",
        "0",
        "00",
        "Ab",
        "a\u{0}",
    ]
    .map(String::from)
    .to_vec();
    corpus.push(format!("a0{}", "1".repeat(MAX_ORDER_KEY_BYTES - 2)));
    corpus.push(format!("a0{}", "1".repeat(MAX_ORDER_KEY_BYTES - 1)));
    for position in [0, 1, 61, 62, 3843, 3844, 238_327] {
        corpus.push(OrderKey::rank(position).unwrap().into_string());
    }

    let mut random = Lcg(0x5eed_0001);
    let mut keys = vec![OrderKey::first()];
    for _ in 0..400 {
        let index = random.below(keys.len());
        let next = match random.below(3) {
            0 => keys[index].key_before(),
            1 => keys[index].key_after(),
            _ => {
                let other = random.below(keys.len());
                let (low, high) = if keys[index] < keys[other] {
                    (&keys[index], &keys[other])
                } else {
                    (&keys[other], &keys[index])
                };
                if low == high {
                    continue;
                }
                OrderKey::between(Some(low), Some(high))
            }
        };
        if let Ok(key) = next {
            keys.push(key);
        }
    }
    corpus.extend(keys.into_iter().map(OrderKey::into_string));

    let alphabet = ['0', '1', 'a', 'b', 'z', 'A', 'Y', 'Z', '-', ' '];
    for _ in 0..2000 {
        let length = random.below(7);
        corpus.push(
            (0..length)
                .map(|_| alphabet[random.below(alphabet.len())])
                .collect(),
        );
    }
    corpus
}

#[test]
fn order_key_rule_agrees_with_the_contract_parser() {
    block_on(async {
        let pool = migrated_pool().await;
        let rule = value_rule("?1", ColumnType::OrderKey);
        for key in order_key_corpus() {
            assert_eq!(
                rule_accepts(&pool, &rule, &key).await,
                OrderKey::parse(&key).is_ok(),
                "order key {key:?}"
            );
        }
    });
}

#[test]
fn row_key_rule_agrees_with_the_contract_type() {
    block_on(async {
        let pool = migrated_pool().await;
        let rule = row_key_rule("?1");
        let mut corpus = [
            "",
            " ",
            "  ",
            "\t",
            "\n",
            "a",
            " a ",
            "note\u{0}",
            "\u{0}",
            "é",
            "note-1",
            "550e8400-e29b-41d4-a716-446655440000",
        ]
        .map(String::from)
        .to_vec();
        corpus.push("x".repeat(MAX_ROW_KEY_BYTES));
        corpus.push("x".repeat(MAX_ROW_KEY_BYTES + 1));
        corpus.push("é".repeat(MAX_ROW_KEY_BYTES / 2));
        corpus.push(format!("{}x", "é".repeat(MAX_ROW_KEY_BYTES / 2)));
        for key in corpus {
            assert_eq!(
                rule_accepts(&pool, &rule, &key).await,
                RowKey::new(&key).is_ok(),
                "row key {key:?}"
            );
        }
    });
}

#[test]
fn rules_reject_values_of_other_types() {
    block_on(async {
        let pool = migrated_pool().await;
        for ty in [
            ColumnType::Text { max_chars: 10 },
            ColumnType::TrimmedText {
                min_chars: 1,
                max_chars: 10,
            },
            ColumnType::Timestamp,
            ColumnType::OrderKey,
        ] {
            let rule = value_rule("?1", ty);
            for value in ["12", "x'6130'"] {
                let sql = format!("SELECT ({}) IS 1", rule.replace("?1", value));
                let accepted: i64 = sqlx::query_scalar(&sql).fetch_one(&pool).await.unwrap();
                assert_eq!(accepted, 0, "{ty:?} accepted {value}");
            }
        }
        for ty in [ColumnType::Flag, ColumnType::Integer { min: 0, max: 31 }] {
            let rule = value_rule("?1", ty);
            for value in ["'1'", "1.0", "NULL", "x'01'"] {
                let sql = format!("SELECT ({}) IS 1", rule.replace("?1", value));
                let accepted: i64 = sqlx::query_scalar(&sql).fetch_one(&pool).await.unwrap();
                assert_eq!(accepted, 0, "{ty:?} accepted {value}");
            }
        }
    });
}

async fn seed(pool: &SqlitePool) {
    execute(
        pool,
        "INSERT INTO quick_note_tags (id, name, order_key) VALUES ('tag-1', 'Work', 'a0');
         INSERT INTO quick_notes (id, order_key) VALUES ('note-1', 'a0');",
    )
    .await;
}

#[test]
fn value_guards_reject_invalid_writes_even_while_applying() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        let long_timestamp = "1".repeat(MAX_TIMESTAMP_BYTES + 1);
        let invalid = [
            "INSERT INTO quick_notes (id, order_key) VALUES (' ', 'a1')".to_string(),
            format!(
                "INSERT INTO quick_notes (id, order_key) VALUES ('{}', 'a1')",
                "x".repeat(MAX_ROW_KEY_BYTES + 1)
            ),
            "INSERT INTO quick_notes (id, order_key) VALUES ('note-2', 'a10')".to_string(),
            "INSERT INTO quick_notes (id, order_key) VALUES (CAST('note' || char(0) AS TEXT), 'a1')"
                .to_string(),
            format!("UPDATE quick_notes SET title = '{}' WHERE id = 'note-1'", "t".repeat(201)),
            "UPDATE quick_notes SET title = x'61' WHERE id = 'note-1'".to_string(),
            "UPDATE quick_notes SET title = 'a' || char(0) || 'b' WHERE id = 'note-1'".to_string(),
            "UPDATE quick_notes SET order_key = 'a' WHERE id = 'note-1'".to_string(),
            "UPDATE quick_notes SET pinned = 'yes' WHERE id = 'note-1'".to_string(),
            "UPDATE quick_notes SET color = 3.5 WHERE id = 'note-1'".to_string(),
            "UPDATE quick_notes SET trashed_at = '' WHERE id = 'note-1'".to_string(),
            format!("UPDATE quick_notes SET updated_at = '{long_timestamp}' WHERE id = 'note-1'"),
            "UPDATE quick_notes SET tag_id = '' WHERE id = 'note-1'".to_string(),
            "UPDATE quick_note_tags SET name = '   ' WHERE id = 'tag-1'".to_string(),
            format!("UPDATE quick_note_tags SET name = '{}' WHERE id = 'tag-1'", "n".repeat(41)),
            "UPDATE quick_note_tags SET order_key = 'a0 ' WHERE id = 'tag-1'".to_string(),
        ];
        for applying in [false, true] {
            set_applying(&pool, applying).await;
            for sql in &invalid {
                assert!(
                    rejects(&pool, sql).await,
                    "accepted while applying={applying}: {sql}"
                );
            }
        }
    });
}

/// Every value the manifest accepts at its bounds is accepted by the schema, so applying a
/// valid operation never fails on a stricter column CHECK.
#[test]
fn schema_accepts_every_manifest_boundary_value() {
    block_on(async {
        let pool = migrated_pool().await;
        seed(&pool).await;
        for table in VAULT_MANIFEST.tables {
            let row = if table.name == "quick_notes" {
                "note-1"
            } else {
                "tag-1"
            };
            for group in table.groups {
                for column in group.columns() {
                    for value in boundary_values(column.ty) {
                        let sql = format!(
                            "UPDATE {table} SET {column} = {value} WHERE {key} = '{row}'",
                            table = table.name,
                            column = column.name,
                            key = table.key_column,
                        );
                        // Each value is tried alone, so cross-column CHECKs see the seeded row.
                        let mut transaction = pool.begin().await.unwrap();
                        sqlx::raw_sql("UPDATE sync_apply_state SET applying = 1")
                            .execute(&mut *transaction)
                            .await
                            .unwrap();
                        let result = sqlx::raw_sql(&sql).execute(&mut *transaction).await;
                        transaction.rollback().await.unwrap();
                        assert!(result.is_ok(), "{sql}: {result:?}");
                    }
                }
            }
        }
        execute(
            &pool,
            &format!(
                "INSERT INTO quick_notes (id, order_key) VALUES ('{}', 'a1')",
                "é".repeat(MAX_ROW_KEY_BYTES / 2)
            ),
        )
        .await;
    });
}

fn boundary_values(ty: ColumnType) -> Vec<String> {
    let quote = |text: &str| format!("'{}'", text.replace('\'', "''"));
    match ty {
        ColumnType::Text { max_chars } => {
            vec![quote(""), quote(&"é".repeat(max_chars)), quote("  ")]
        }
        ColumnType::TrimmedText {
            min_chars,
            max_chars,
        } => vec![
            quote(&"w".repeat(min_chars)),
            quote(&format!(" {} ", "é".repeat(max_chars))),
        ],
        ColumnType::Integer { min, max } => vec![min.to_string(), max.to_string()],
        ColumnType::Flag => vec!["0".to_string(), "1".to_string()],
        ColumnType::Timestamp => vec![quote(&"9".repeat(MAX_TIMESTAMP_BYTES)), quote("x")],
        ColumnType::OptionalTimestamp => {
            vec!["NULL".to_string(), quote(&"9".repeat(MAX_TIMESTAMP_BYTES))]
        }
        ColumnType::OrderKey => vec![
            quote("a0"),
            quote(&format!("a0{}", "z".repeat(MAX_ORDER_KEY_BYTES - 2))),
            quote("A000000000000000000000000001"),
        ],
        ColumnType::OptionalRowKey => vec!["NULL".to_string(), quote("tag-1")],
    }
}

//! The first-open import of preferences that `config.json` still holds.
//!
//! The steps are ordered and each is idempotent, since a file write cannot join the SQLite
//! transaction: portable values are inserted without replacing existing rows, device values
//! fill only absent entries, and `config.json` is reduced to its vault config branches last.
//! A crash between steps repeats the import at the next open with no effect on what landed.

use super::registry::{self, Scope, Shape};
use super::view::{PortableRow, value_at};
use serde_json::{Map, Value};
use sqlx::SqlitePool;

/// The registered portable values of `config`, as row keys and canonical JSON text. Values
/// that fail their rule are skipped, so they read as defaults after the import.
pub(crate) fn portable_entries(config: &Value) -> Vec<PortableRow> {
    let mut entries = Vec::new();
    for spec in registry::specs(Scope::Portable) {
        let Some(value) = value_at(config, spec.path) else {
            continue;
        };
        match spec.shape {
            Shape::Exact => {
                if let Some(text) = canonical(spec.rule.accepts(value), value) {
                    entries.push((spec.path.to_owned(), text));
                }
            }
            Shape::PerChild { max_children } => {
                let Some(children) = value.as_object() else {
                    continue;
                };
                let accepted = children.iter().filter_map(|(child, value)| {
                    registry::validate_child_key(spec, child).ok()?;
                    let text = canonical(spec.rule.accepts(value), value)?;
                    Some((spec.child_key(child), text))
                });
                entries.extend(accepted.take(usize::from(max_children)));
            }
        }
    }
    entries
}

/// The registered device values of `config`, keyed by registry path.
pub(crate) fn device_entries(config: &Value) -> Map<String, Value> {
    registry::specs(Scope::Device)
        .filter_map(|spec| {
            let value = value_at(config, spec.path)?;
            spec.rule
                .accepts(value)
                .then(|| (spec.path.to_owned(), value.clone()))
        })
        .collect()
}

/// Whether `config` holds anything besides its vault config branches.
pub(crate) fn has_moved_branches(config: &Value) -> bool {
    config
        .as_object()
        .is_some_and(|root| root.keys().any(|key| !is_vault_config_branch(key)))
}

/// Removes every top-level branch of `root` that is not a vault config branch.
pub(crate) fn retain_vault_config_branches(root: &mut Value) {
    if let Some(root) = root.as_object_mut() {
        root.retain(|key, _| is_vault_config_branch(key));
    }
}

fn is_vault_config_branch(key: &str) -> bool {
    registry::specs(Scope::VaultConfig).any(|spec| spec.path == key)
}

/// Inserts `entries` into `vault_preferences` in one transaction, keeping rows that exist.
pub(crate) async fn insert_missing_rows(
    pool: &SqlitePool,
    entries: &[PortableRow],
    updated_at: &str,
) -> Result<(), String> {
    if entries.is_empty() {
        return Ok(());
    }
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| format!("begin preference import: {error}"))?;
    for (key, value) in entries {
        sqlx::query(
            "INSERT OR IGNORE INTO vault_preferences (key, value, updated_at) VALUES (?, ?, ?)",
        )
        .bind(key)
        .bind(value)
        .bind(updated_at)
        .execute(&mut *transaction)
        .await
        .map_err(|error| format!("import preference {key}: {error}"))?;
    }
    transaction
        .commit()
        .await
        .map_err(|error| format!("commit preference import: {error}"))
}

/// Adds the entries the device file does not have yet. Returns whether it changed.
pub(crate) fn merge_missing_device_entries(
    existing: &mut Map<String, Value>,
    entries: Map<String, Value>,
) -> bool {
    let mut changed = false;
    for (key, value) in entries {
        if !existing.contains_key(&key) {
            existing.insert(key, value);
            changed = true;
        }
    }
    changed
}

fn canonical(accepted: bool, value: &Value) -> Option<String> {
    if !accepted {
        return None;
    }
    serde_json::to_string(value)
        .ok()
        .filter(|text| text.len() <= registry::MAX_VALUE_BYTES)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::preferences::tests::{memory_pool, rows};
    use serde_json::json;

    const NOW: &str = "2026-10-22T10:51:41.000Z";
    const LATER: &str = "2026-10-23T00:00:00.000Z";

    fn legacy_config() -> Value {
        json!({
            "profile": { "displayName": "Ada", "color": 99 },
            "preferences": { "language": "es", "fontScale": 1.2, "unknownKey": 1 },
            "notes": { "customOrderByProject": { "p1": ["a"], "": ["b"], "p2": "bad" } },
            "theme": { "activeId": "dark", "quickToggleDarkId": "night" },
            "distractions": { "rules": [] },
            "chat": { "schemaVersion": 1 },
        })
    }

    #[test]
    fn preferences_import_extracts_valid_portable_and_device_values() {
        let config = legacy_config();
        let mut portable = portable_entries(&config);
        portable.sort();
        assert_eq!(
            portable,
            [
                ("notes.customOrderByProject.p1", "[\"a\"]"),
                ("preferences.language", "\"es\""),
                ("profile.displayName", "\"Ada\""),
                ("theme.quickToggleDarkId", "\"night\""),
            ]
            .map(|(key, value)| (key.to_owned(), value.to_owned()))
        );
        assert_eq!(
            Value::Object(device_entries(&config)),
            json!({ "preferences.fontScale": 1.2, "theme.activeId": "dark" })
        );
    }

    #[tokio::test]
    async fn preferences_import_is_fresh_then_a_no_op_on_repeat() {
        let pool = memory_pool().await;
        let config = legacy_config();
        insert_missing_rows(&pool, &portable_entries(&config), NOW)
            .await
            .unwrap();
        assert_eq!(rows(&pool).await.len(), 4);

        sqlx::query(
            "UPDATE vault_preferences SET value = '\"en\"' WHERE key = 'preferences.language'",
        )
        .execute(&pool)
        .await
        .unwrap();
        insert_missing_rows(&pool, &portable_entries(&config), LATER)
            .await
            .unwrap();
        let repeated = rows(&pool).await;
        assert_eq!(repeated.len(), 4);
        assert!(repeated.contains(&("preferences.language".to_owned(), "\"en\"".to_owned())));
    }

    #[test]
    fn preferences_import_fills_only_absent_device_entries_and_strips_config() {
        let config = legacy_config();
        let mut existing = json!({ "theme.activeId": "light" })
            .as_object()
            .cloned()
            .unwrap();
        assert!(merge_missing_device_entries(
            &mut existing,
            device_entries(&config)
        ));
        assert_eq!(existing["theme.activeId"], "light");
        assert_eq!(existing["preferences.fontScale"], 1.2);
        assert!(!merge_missing_device_entries(
            &mut existing,
            device_entries(&config)
        ));

        let mut stripped = config.clone();
        assert!(has_moved_branches(&stripped));
        retain_vault_config_branches(&mut stripped);
        assert_eq!(
            stripped,
            json!({ "distractions": { "rules": [] }, "chat": { "schemaVersion": 1 } })
        );
        assert!(!has_moved_branches(&stripped));
        assert!(portable_entries(&stripped).is_empty());
    }
}

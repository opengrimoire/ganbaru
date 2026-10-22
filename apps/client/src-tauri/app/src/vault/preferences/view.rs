//! Composition of the effective preference view from portable rows and device entries.

use super::registry::{self, PATH_SEPARATOR};
use serde_json::{Map, Value};
use sqlx::SqlitePool;

/// One stored portable row: its key and canonical JSON text.
pub(crate) type PortableRow = (String, String);

/// Reads every portable row.
pub(crate) async fn load_portable_rows(pool: &SqlitePool) -> Result<Vec<PortableRow>, String> {
    sqlx::query_as::<_, PortableRow>("SELECT key, value FROM vault_preferences ORDER BY key")
        .fetch_all(pool)
        .await
        .map_err(|error| format!("read portable preferences: {error}"))
}

/// Builds the nested preference object from the valid portable rows and device entries.
/// Unknown keys and values that fail their rule are left out, so readers see the default.
pub(crate) fn compose(rows: &[PortableRow], device: &Map<String, Value>) -> Value {
    let mut root = Map::new();
    for (key, text) in rows {
        let Some((spec, child)) = registry::portable_row(key) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<Value>(text) else {
            continue;
        };
        if spec.rule.accepts(&value) {
            let path = spec.segments().map(|segment| segment as &str);
            insert_at(&mut root, path.chain(child), value);
        }
    }
    for (key, value) in device {
        if let Some(spec) = registry::device_spec(key)
            && spec.rule.accepts(value)
        {
            insert_at(&mut root, spec.segments(), value.clone());
        }
    }
    Value::Object(root)
}

/// Sets `value` at the nested path, creating objects along the way.
pub(super) fn insert_at<'a>(
    root: &mut Map<String, Value>,
    segments: impl Iterator<Item = &'a str>,
    value: Value,
) {
    let segments = segments.collect::<Vec<_>>();
    let Some((leaf, parents)) = segments.split_last() else {
        return;
    };
    let mut current = root;
    for segment in parents {
        let next = current
            .entry((*segment).to_owned())
            .or_insert_with(|| Value::Object(Map::new()));
        if !next.is_object() {
            *next = Value::Object(Map::new());
        }
        let Value::Object(map) = next else {
            return;
        };
        current = map;
    }
    current.insert((*leaf).to_owned(), value);
}

/// The value at a dotted registry path of the view, if present.
pub(crate) fn value_at<'a>(view: &'a Value, path: &str) -> Option<&'a Value> {
    path.split(PATH_SEPARATOR)
        .try_fold(view, |current, segment| current.get(segment))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(key: &str, value: &str) -> PortableRow {
        (key.to_owned(), value.to_owned())
    }

    #[test]
    fn preferences_view_nests_valid_rows_and_device_entries() {
        let rows = [
            row("profile.color", "7"),
            row("preferences.language", "\"es\""),
            row("notes.customOrderByProject.project-1", "[\"a\",\"b\"]"),
            row("notes.customOrderByProject.project.2", "[\"c\"]"),
        ];
        let device = json!({ "theme.activeId": "dark", "preferences.fontScale": 1.1 });
        let view = compose(&rows, device.as_object().unwrap());
        assert_eq!(
            view,
            json!({
                "profile": { "color": 7 },
                "preferences": { "language": "es", "fontScale": 1.1 },
                "notes": { "customOrderByProject": {
                    "project-1": ["a", "b"],
                    "project.2": ["c"],
                } },
                "theme": { "activeId": "dark" },
            })
        );
        assert_eq!(value_at(&view, "theme.activeId"), Some(&json!("dark")));
        assert_eq!(value_at(&view, "theme.quickToggleDarkId"), None);
    }

    #[test]
    fn preferences_view_reads_invalid_and_unknown_values_as_defaults() {
        let rows = [
            row("profile.color", "99"),
            row("preferences.calendarTimeFormat", "\"36h\""),
            row("preferences.futureKey", "true"),
            row("profile.displayName", "not json"),
            row("theme.activeId", "\"dark\""),
        ];
        let device = json!({
            "preferences.fontScale": 4,
            "profile.color": 3,
            "unknown": true,
        });
        assert_eq!(compose(&rows, device.as_object().unwrap()), json!({}));
    }
}

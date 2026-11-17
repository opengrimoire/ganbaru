//! Preference reads and edits of a TOML configuration file that keep its comments, order, and
//! formatting, as Codex and Grok read `config.toml`.

use serde_json::Value;
use toml_edit::{DocumentMut, InlineTable, Item, Table};

use super::PreferenceDocument;

/// A parsed TOML document.
pub(super) struct TomlDocument(DocumentMut);

impl PreferenceDocument for TomlDocument {
    fn parse(contents: &str) -> Option<Self> {
        contents.parse::<DocumentMut>().ok().map(Self)
    }

    fn value_at(&self, path: &[&str]) -> Option<Value> {
        let mut item = self.0.as_item();
        for key in path {
            item = item.as_table_like()?.get(key)?;
        }
        Some(json_value(item))
    }

    fn insert_at(&mut self, path: &[&str], value: Value) -> bool {
        let Some((leaf, parents)) = path.split_last() else {
            return false;
        };
        let Some(stored) = toml_value(&value) else {
            return false;
        };
        if path_blocked(self.0.as_item(), parents, leaf) {
            return false;
        }
        let mut item = self.0.as_item_mut();
        for key in parents {
            let child = new_child(item);
            let missing = item
                .as_table_like()
                .is_some_and(|table| !table.contains_key(key));
            if missing && !insert_entry(item, key, child) {
                return false;
            }
            let Some(next) = item
                .as_table_like_mut()
                .and_then(|table| table.get_mut(key))
            else {
                return false;
            };
            item = next;
        }
        match item
            .as_table_like_mut()
            .and_then(|table| table.get_mut(leaf))
        {
            Some(Item::Value(existing)) => {
                // Keeps the spacing and the comment that follow the old value.
                let decor = existing.decor().clone();
                *existing = stored;
                *existing.decor_mut() = decor;
                true
            }
            Some(_) => false,
            None => insert_entry(item, leaf, Item::Value(stored)),
        }
    }

    fn remove_at(&mut self, path: &[&str]) {
        remove_from(self.0.as_item_mut(), path);
    }

    fn render(self) -> Option<String> {
        Some(self.0.to_string())
    }
}

/// Adds `entry` under `key` of the table `item`. In an inline table the spacing before the
/// closing brace moves from the old last entry to the new one, so `{ a = 1 }` becomes
/// `{ a = 1, b = 2 }`.
fn insert_entry(item: &mut Item, key: &str, entry: Item) -> bool {
    if let Some(inline) = item.as_inline_table_mut() {
        let Item::Value(mut value) = entry else {
            return false;
        };
        if let Some((_, last)) = inline.iter_mut().last() {
            if let Some(suffix) = last.decor().suffix().cloned() {
                value.decor_mut().set_suffix(suffix);
            }
            last.decor_mut().set_suffix("");
        }
        inline.insert(key, value);
        return true;
    }
    match item.as_table_like_mut() {
        Some(table) => {
            table.insert(key, entry);
            true
        }
        None => false,
    }
}

/// Whether a parent along `parents` exists and is not a table, or `leaf` holds a table or an
/// array.
fn path_blocked(root: &Item, parents: &[&str], leaf: &str) -> bool {
    let mut item = root;
    for key in parents {
        let Some(table) = item.as_table_like() else {
            return true;
        };
        match table.get(key) {
            None => return false,
            Some(child) => item = child,
        }
    }
    match item.as_table_like() {
        Some(table) => table.get(leaf).is_some_and(|child| {
            child
                .as_value()
                .is_none_or(|value| value.is_inline_table() || value.is_array())
        }),
        None => true,
    }
}

/// An empty table of the kind `parent` holds: an inline table inside an inline table, a dotted
/// table inside a dotted one, and otherwise a table whose header appears once it has keys.
fn new_child(parent: &Item) -> Item {
    if parent.is_inline_table() {
        return Item::Value(toml_edit::Value::InlineTable(InlineTable::new()));
    }
    let mut table = Table::new();
    table.set_implicit(true);
    table.set_dotted(parent.as_table().is_some_and(Table::is_dotted));
    Item::Table(table)
}

/// Removes the value at `path` and every parent table the removal leaves empty.
fn remove_from(item: &mut Item, path: &[&str]) {
    let Some((first, rest)) = path.split_first() else {
        return;
    };
    let Some(table) = item.as_table_like_mut() else {
        return;
    };
    if rest.is_empty() {
        table.remove(first);
        return;
    }
    let Some(child) = table.get_mut(first) else {
        return;
    };
    if !child.is_table_like() {
        return;
    }
    remove_from(child, rest);
    if child.as_table_like().is_some_and(|child| child.is_empty()) {
        table.remove(first);
    }
}

/// The JSON form of a TOML item that preference checks read. Tables, arrays, dates, and
/// non-finite floats become values no check accepts, so they read as set but ignored.
fn json_value(item: &Item) -> Value {
    let Some(value) = item.as_value() else {
        return Value::Object(serde_json::Map::new());
    };
    match value {
        toml_edit::Value::Boolean(value) => Value::Bool(*value.value()),
        toml_edit::Value::Integer(value) => Value::from(*value.value()),
        toml_edit::Value::String(value) => Value::String(value.value().clone()),
        toml_edit::Value::Float(value) => {
            serde_json::Number::from_f64(*value.value()).map_or(Value::Null, Value::Number)
        }
        toml_edit::Value::Datetime(_)
        | toml_edit::Value::Array(_)
        | toml_edit::Value::InlineTable(_) => Value::Array(Vec::new()),
    }
}

/// The TOML value for a checked preference value, which is a boolean, a count, or a string.
fn toml_value(value: &Value) -> Option<toml_edit::Value> {
    match value {
        Value::Bool(value) => Some((*value).into()),
        Value::Number(number) => number.as_i64().map(Into::into),
        Value::String(text) => Some(text.as_str().into()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn edited(contents: &str, edit: impl FnOnce(&mut TomlDocument)) -> String {
        let mut document = TomlDocument::parse(contents).unwrap();
        edit(&mut document);
        document.render().unwrap()
    }

    #[test]
    fn edits_keep_comments_order_and_other_tables() {
        let contents = "# Codex settings\nmodel = \"gpt\" # picked in Ganbaru\nhide_agent_reasoning = false # keep it\n\n[mcp_servers.docs]\ncommand = \"docs\"\n";
        let result = edited(contents, |document| {
            assert!(document.insert_at(&["hide_agent_reasoning"], json!(true)));
            assert!(document.insert_at(&["model_verbosity"], json!("low")));
            assert!(document.insert_at(&["history", "persistence"], json!("none")));
        });
        assert_eq!(
            result,
            "# Codex settings\nmodel = \"gpt\" # picked in Ganbaru\nhide_agent_reasoning = true # keep it\nmodel_verbosity = \"low\"\n\n[mcp_servers.docs]\ncommand = \"docs\"\n\n[history]\npersistence = \"none\"\n"
        );
    }

    #[test]
    fn nested_tables_show_only_the_header_that_holds_keys() {
        let result = edited("", |document| {
            assert!(document.insert_at(&["toolset", "bash", "timeout_secs"], json!(300)));
        });
        assert_eq!(result, "[toolset.bash]\ntimeout_secs = 300\n");
        let result = edited(&result, |document| {
            document.remove_at(&["toolset", "bash", "timeout_secs"]);
        });
        assert_eq!(result, "");
    }

    #[test]
    fn dotted_keys_and_inline_tables_stay_in_their_form() {
        let result = edited(
            "session.load_envrc = true\nui = { compact_mode = true }\n",
            |document| {
                assert!(
                    document.insert_at(&["session", "auto_compact_threshold_percent"], json!(70))
                );
                assert!(document.insert_at(&["ui", "show_thinking_blocks"], json!(false)));
            },
        );
        assert_eq!(
            result,
            "session.load_envrc = true\nsession.auto_compact_threshold_percent = 70\nui = { compact_mode = true, show_thinking_blocks = false }\n"
        );
        let parsed = TomlDocument::parse(&result).unwrap();
        assert_eq!(
            parsed.value_at(&["session", "auto_compact_threshold_percent"]),
            Some(json!(70))
        );
        assert_eq!(
            parsed.value_at(&["ui", "show_thinking_blocks"]),
            Some(json!(false))
        );
    }

    #[test]
    fn removals_drop_empty_parent_tables_but_keep_others() {
        let contents =
            "[history]\npersistence = \"none\"\n\n[analytics]\nenabled = false\nnote = \"kept\"\n";
        let result = edited(contents, |document| {
            document.remove_at(&["history", "persistence"]);
            document.remove_at(&["analytics", "enabled"]);
            document.remove_at(&["missing", "key"]);
        });
        // The blank line that separated the removed table stays.
        assert_eq!(result, "\n[analytics]\nnote = \"kept\"\n");
    }

    #[test]
    fn values_in_the_way_block_edits_without_changes() {
        let contents =
            "history = \"all\"\n[[tools]]\nname = \"a\"\n[feedback]\nenabled = { nested = true }\n";
        let mut document = TomlDocument::parse(contents).unwrap();
        assert!(!document.insert_at(&["history", "persistence"], json!("none")));
        assert!(!document.insert_at(&["tools", "web_search"], json!(true)));
        assert!(!document.insert_at(&["feedback", "enabled"], json!(true)));
        assert_eq!(document.render().unwrap(), contents);
    }

    #[test]
    fn reads_convert_scalars_and_mark_other_values() {
        let document = TomlDocument::parse(
            "a = true\nb = 42\nc = \"text\"\nd = 1.5\ne = [1]\nf = 1979-05-27\n[g]\n",
        )
        .unwrap();
        assert_eq!(document.value_at(&["a"]), Some(json!(true)));
        assert_eq!(document.value_at(&["b"]), Some(json!(42)));
        assert_eq!(document.value_at(&["c"]), Some(json!("text")));
        assert_eq!(document.value_at(&["d"]), Some(json!(1.5)));
        assert_eq!(document.value_at(&["e"]), Some(json!([])));
        assert_eq!(document.value_at(&["f"]), Some(json!([])));
        assert_eq!(document.value_at(&["g"]), Some(json!({})));
        assert_eq!(document.value_at(&["b", "x"]), None);
        assert_eq!(document.value_at(&["missing"]), None);
        assert!(TomlDocument::parse("a = ").is_none());
    }
}

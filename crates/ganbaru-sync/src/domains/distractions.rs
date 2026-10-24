//! Distraction rules adapter: value checks the column types cannot express, recovery titles,
//! and the hook that reports rule changes so the app republishes what its blockers read.

use ganbaru_sync_contracts::{Field, GroupId, TableId, Value};
use sqlx::SqliteConnection;

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::hooks::{ChangeSink, HookFuture, Hooks, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::distractions::{
    ADAPTER, APP_RULES_TABLE, CUSTOM_CATEGORIES_TABLE, DOMAIN, HOST_RULES_TABLE, SETTINGS_TABLE,
    USAGE_LIMITS_TABLE, app_group, custom_category_group, host_group, limit_group, settings_group,
};

/// Most characters of an enumerated value: a mode, list kind, or platform.
pub const MAX_ENUM_CHARS: usize = 16;

/// Most characters of a category, custom category, limit, or limit entry id.
pub const MAX_ID_CHARS: usize = 80;

/// Most characters of a host name.
pub const MAX_HOST_CHARS: usize = 253;

/// Most characters of a custom category name.
pub const MAX_CATEGORY_NAME_CHARS: usize = 60;

/// Most characters of an app name or a limit entry name.
pub const MAX_APP_NAME_CHARS: usize = 80;

/// Most characters of a usage limit or limit entry name.
pub const MAX_LIMIT_NAME_CHARS: usize = 80;

/// Most characters of an Android package name.
pub const MAX_PACKAGE_CHARS: usize = 255;

/// Most characters of an app rule identity: a package name or a lowercase desktop name.
pub const MAX_APP_IDENTITY_CHARS: usize = MAX_PACKAGE_CHARS;

/// Most desktop process names one app rule or limit entry matches.
pub const MAX_MATCH_NAMES: usize = 64;

/// Most characters of the JSON array of desktop match names: every name escaped in full.
pub const MAX_MATCH_NAMES_JSON_CHARS: usize = 2 + MAX_MATCH_NAMES * (2 * MAX_APP_NAME_CHARS + 3);

/// Largest event palette index of a limit entry color.
pub const MAX_COLOR: i64 = 31;

/// Most entries of one usage limit, the native limit on entries of every limit together.
pub const MAX_LIMIT_ENTRIES: usize = 2_000;

/// Largest daily budget in minutes.
pub const MAX_MINUTES_PER_DAY: i64 = 1_440;

/// Largest weekly budget in minutes.
pub const MAX_MINUTES_PER_WEEK: i64 = 10_080;

/// Change notice topic of a replicated rule table; the notice key is the table name.
pub const RULES_TOPIC: &str = "distractions_rules";

/// Blocking modes of the settings row.
pub const MODES: &[&str] = &["blacklist", "whitelist"];

/// Host lists: the three global lists and the hosts of a custom category.
pub const HOST_LIST_KINDS: &[&str] = &["blocked", "exception", "allowed", "category"];

/// List kind of a custom category host, whose scope is the category id.
pub const CATEGORY_LIST_KIND: &str = "category";

/// Platforms of an app rule.
pub const APP_PLATFORMS: &[&str] = &["desktop", "mobile"];

/// Whether `host` is a normalized host rule: lowercase ASCII labels of letters, digits, and
/// inner hyphens joined by dots, as rule normalization writes them.
pub fn is_rule_host(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= MAX_HOST_CHARS
        && host.split('.').all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
}

/// The desktop match names of a JSON array, or `None` when the text is not an array of at most
/// [`MAX_MATCH_NAMES`] non-blank names.
pub fn parse_match_names(json: &str) -> Option<Vec<String>> {
    let names: Vec<String> = serde_json::from_str(json).ok()?;
    let valid = names.len() <= MAX_MATCH_NAMES
        && names
            .iter()
            .all(|name| !name.trim().is_empty() && name.chars().count() <= MAX_APP_NAME_CHARS);
    valid.then_some(names)
}

fn one_of(field: Option<&Field>, allowed: &[&str], what: &'static str) -> Result<(), AdapterError> {
    match field {
        Some(Field::Text(text)) if allowed.contains(&text.as_str()) => Ok(()),
        _ => Err(AdapterError(what)),
    }
}

fn validate_host_rule(value: &Value) -> Result<(), AdapterError> {
    let [Field::Text(kind), Field::Text(scope), Field::Text(host)] = value.fields() else {
        return Err(AdapterError("distraction host rule"));
    };
    if !HOST_LIST_KINDS.contains(&kind.as_str()) {
        return Err(AdapterError("distraction host list"));
    }
    if (kind == CATEGORY_LIST_KIND) == scope.is_empty() {
        return Err(AdapterError("distraction host scope"));
    }
    if !is_rule_host(host) {
        return Err(AdapterError("distraction host"));
    }
    Ok(())
}

/// Adapter of the distraction rules domain.
#[derive(Debug, Default, Clone, Copy)]
pub struct DistractionsAdapter;

impl DomainAdapter for DistractionsAdapter {
    fn name(&self) -> &'static str {
        ADAPTER
    }

    fn derived(
        &self,
        _table: TableId,
        _group: GroupId,
        _value: &Value,
    ) -> Result<Vec<(&'static str, Field)>, AdapterError> {
        Ok(Vec::new())
    }

    /// Coupled rule groups are toggles with no preferred side, so clocks decide.
    fn priority(&self, _table: TableId, _group: GroupId, _value: &Value) -> u8 {
        0
    }

    fn validate(&self, table: TableId, group: GroupId, value: &Value) -> Result<(), AdapterError> {
        let fields = value.fields();
        if table == SETTINGS_TABLE && group == settings_group::MODE {
            one_of(fields.first(), MODES, "distraction mode")
        } else if table == HOST_RULES_TABLE && group == host_group::RULE {
            validate_host_rule(value)
        } else if table == APP_RULES_TABLE && group == app_group::RULE {
            one_of(fields.first(), APP_PLATFORMS, "distraction app platform")
        } else if table == APP_RULES_TABLE && group == app_group::LABEL {
            match fields.get(1) {
                Some(Field::Text(json)) if parse_match_names(json).is_some() => Ok(()),
                _ => Err(AdapterError("distraction app match names")),
            }
        } else if table == USAGE_LIMITS_TABLE && group == limit_group::BUDGET {
            match fields {
                [Field::Null, Field::Null] => Err(AdapterError("distraction limit budget")),
                _ => Ok(()),
            }
        } else {
            Ok(())
        }
    }

    /// Recovery titles of custom categories and usage limits come from their names.
    fn presentation(&self, table: TableId, values: &[(GroupId, Value)]) -> Presentation {
        let name_group = if table == CUSTOM_CATEGORIES_TABLE {
            custom_category_group::NAME
        } else if table == USAGE_LIMITS_TABLE {
            limit_group::NAME
        } else {
            return Presentation::default();
        };
        let mut presentation = Presentation::default();
        for (group, value) in values {
            if *group == name_group
                && let [Field::Text(name)] = value.fields()
            {
                presentation.title = name.trim_matches(' ').to_string();
            }
        }
        presentation
    }
}

/// Reports a rule table a materialization changed under [`RULES_TOPIC`], once per table, so
/// the app reassembles the rules and republishes what its blockers read.
#[derive(Debug, Default, Clone, Copy)]
pub struct DistractionsHook;

impl MaterializeHook for DistractionsHook {
    fn after_materialize<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            if !changed_keys.is_empty() {
                changes.report(RULES_TOPIC, table.name.to_string());
            }
            Ok(())
        })
    }
}

/// `hooks` with the rules hook on every table of the domain.
pub fn with_hooks(hooks: Hooks) -> Hooks {
    DOMAIN.tables.iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(DistractionsHook))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::vault::distractions::CATEGORY_RULES_TABLE;

    fn text(value: &str) -> Field {
        Field::Text(value.to_string())
    }

    fn value(fields: Vec<Field>) -> Value {
        Value::new(fields).unwrap()
    }

    #[test]
    fn distractions_validation_refuses_values_rule_writers_never_produce() {
        let adapter = DistractionsAdapter;
        let host = |kind: &str, scope: &str, host: &str| {
            adapter.validate(
                HOST_RULES_TABLE,
                host_group::RULE,
                &value(vec![text(kind), text(scope), text(host)]),
            )
        };
        assert_eq!(host("blocked", "", "example.com"), Ok(()));
        assert_eq!(host("category", "focus-sites", "news.example.org"), Ok(()));
        assert!(host("category", "", "example.com").is_err());
        assert!(host("allowed", "focus-sites", "example.com").is_err());
        assert!(host("muted", "", "example.com").is_err());
        assert!(host("blocked", "", "Example.com").is_err());
        assert!(host("blocked", "", "example..com").is_err());
        assert!(host("blocked", "", "-example.com").is_err());

        let mode = |mode: &str| {
            adapter.validate(
                SETTINGS_TABLE,
                settings_group::MODE,
                &value(vec![text(mode)]),
            )
        };
        assert_eq!(mode("whitelist"), Ok(()));
        assert!(mode("graylist").is_err());

        let label = |json: &str| {
            adapter.validate(
                APP_RULES_TABLE,
                app_group::LABEL,
                &value(vec![text("Steam"), text(json)]),
            )
        };
        assert_eq!(label(r#"["Steam","steamwebhelper"]"#), Ok(()));
        assert_eq!(label("[]"), Ok(()));
        assert!(label(r#"[" "]"#).is_err());
        assert!(label(r#"{"name":"Steam"}"#).is_err());
        let too_many = serde_json::to_string(&vec!["app"; MAX_MATCH_NAMES + 1]).unwrap();
        assert!(label(&too_many).is_err());

        let budget = |day: Field, week: Field| {
            adapter.validate(
                USAGE_LIMITS_TABLE,
                limit_group::BUDGET,
                &value(vec![day, week]),
            )
        };
        assert_eq!(budget(Field::Integer(30), Field::Null), Ok(()));
        assert_eq!(budget(Field::Null, Field::Integer(300)), Ok(()));
        assert!(budget(Field::Null, Field::Null).is_err());
    }

    #[test]
    fn distractions_recovery_titles_name_categories_and_limits() {
        let adapter = DistractionsAdapter;
        let named = |table, group| {
            adapter
                .presentation(table, &[(group, value(vec![text(" Games ")]))])
                .title
        };
        assert_eq!(
            named(CUSTOM_CATEGORIES_TABLE, custom_category_group::NAME),
            "Games"
        );
        assert_eq!(named(USAGE_LIMITS_TABLE, limit_group::NAME), "Games");
        assert_eq!(named(CATEGORY_RULES_TABLE, custom_category_group::NAME), "");
    }
}

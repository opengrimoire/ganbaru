//! Distraction rules and usage adapter: value checks the column types cannot express, recovery
//! titles, and the hook that reports rule changes so the app republishes what its blockers read.

use ganbaru_sync_contracts::{Field, GroupId, GroupValue, TableId, Value};
use sqlx::SqliteConnection;

use crate::adapter::{AdapterError, DomainAdapter, Presentation};
use crate::codec;
use crate::hooks::{ChangeSink, HookFuture, Hooks, MaterializeHook};
use crate::manifest::TableSpec;
use crate::manifest::vault::distractions::{
    ADAPTER, APP_RULES_TABLE, BLOCK_EVENTS_TABLE, BLOCK_RULE_SNAPSHOTS, CUSTOM_CATEGORIES_TABLE,
    DOMAIN, HOST_RULES_TABLE, SETTINGS_TABLE, USAGE_LIMITS_TABLE, USAGE_TOTALS_TABLE, app_group,
    block_event_group, custom_category_group, host_group, limit_group, settings_group,
    usage_total_group,
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

/// Most characters of a usage source key: a host, a desktop app name, or a package name.
pub const MAX_USAGE_SOURCE_KEY_CHARS: usize = MAX_PACKAGE_CHARS;

/// Most characters of a usage display name, as sample normalization truncates it.
pub const MAX_USAGE_LABEL_CHARS: usize = 120;

/// Characters of a `yyyy-mm-dd` local date.
pub const USAGE_DATE_CHARS: usize = 10;

/// Most seconds one usage total holds: a 25-hour local day, which a daylight saving change can
/// produce. Overlapping samples cannot push a total past one day of wall time.
pub const MAX_USAGE_DAY_SECONDS: i64 = 25 * 60 * 60;

/// Usage source types: a website host, a desktop app, or a mobile app package.
pub const USAGE_SOURCE_TYPES: &[&str] = &["website", "desktop-app", "mobile-app"];

/// Most characters of an enumerated block event or rule snapshot value.
pub const MAX_BLOCK_ENUM_CHARS: usize = 32;

/// Most characters of a free block event or rule snapshot text: a source key, label, or id.
/// One snapshot row of every such column stays within one blob field.
pub const MAX_BLOCK_TEXT_CHARS: usize = 1_024;

/// Block event source types.
pub const BLOCK_SOURCE_TYPES: &[&str] = &["browser", "desktop_app", "mobile_app"];

/// Focus phases a block event records, including the pause kinds.
pub const BLOCK_PHASES: &[&str] = &[
    "focus",
    "short_break",
    "long_break",
    "manual_pause",
    "idle_pause",
    "suspend_pause",
];

/// Block event decisions.
pub const BLOCK_DECISIONS: &[&str] = &[
    "blocked",
    "temporary_allowed",
    "false_positive_reported",
    "limit_exhausted",
];

/// Rule kinds a block event's rule snapshot names.
pub const SNAPSHOT_RULE_KINDS: &[&str] = &[
    "domain",
    "url_pattern",
    "category",
    "custom_category",
    "usage_limit",
    "desktop_app",
    "mobile_app",
];

/// Blocker modes a block event's rule snapshot names.
pub const SNAPSHOT_BLOCKER_MODES: &[&str] = &["blacklist", "whitelist", "limit"];

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

/// Whether `date` is a `yyyy-mm-dd` date with a month of 1 to 12 and a day of 1 to 31, the form
/// usage writers store. Days past a month's end are left to the writer's calendar check.
fn is_usage_date(date: &str) -> bool {
    let bytes = date.as_bytes();
    let digits = |range: std::ops::Range<usize>| {
        bytes[range.clone()]
            .iter()
            .all(u8::is_ascii_digit)
            .then(|| {
                bytes[range]
                    .iter()
                    .fold(0_u32, |value, digit| value * 10 + u32::from(digit - b'0'))
            })
    };
    if bytes.len() != USAGE_DATE_CHARS || bytes[4] != b'-' || bytes[7] != b'-' {
        return false;
    }
    matches!(
        (digits(0..4), digits(5..7), digits(8..10)),
        (Some(_), Some(1..=12), Some(1..=31))
    )
}

fn optional_one_of(
    field: Option<&Field>,
    allowed: &[&str],
    what: &'static str,
) -> Result<(), AdapterError> {
    match field {
        Some(Field::Null) => Ok(()),
        field => one_of(field, allowed, what),
    }
}

/// The source type, phase, and decision of a block event, and a source key that is never a URL.
fn validate_block_event(value: &Value) -> Result<(), AdapterError> {
    let [
        _,
        source_type,
        Field::Text(source_key),
        _,
        phase,
        decision,
        _,
        _,
        _,
    ] = value.fields()
    else {
        return Err(AdapterError("distraction block event"));
    };
    one_of(
        Some(source_type),
        BLOCK_SOURCE_TYPES,
        "distraction block source type",
    )?;
    if source_key.contains("://") {
        return Err(AdapterError("distraction block source key"));
    }
    optional_one_of(Some(phase), BLOCK_PHASES, "distraction block phase")?;
    one_of(
        Some(decision),
        BLOCK_DECISIONS,
        "distraction block decision",
    )
}

/// The rule kind and blocker mode of a block event's rule snapshot.
fn validate_block_snapshot(value: &Value) -> Result<(), AdapterError> {
    let [Field::Blob(bytes)] = value.fields() else {
        return Err(AdapterError("distraction block rule snapshot"));
    };
    let rows = codec::decode(&BLOCK_RULE_SNAPSHOTS, bytes)
        .map_err(|_| AdapterError("distraction block rule snapshot"))?;
    rows.iter().try_for_each(|row| match row.fields.as_slice() {
        [_, kind, _, _, mode] => {
            optional_one_of(Some(kind), SNAPSHOT_RULE_KINDS, "distraction rule kind")?;
            optional_one_of(
                Some(mode),
                SNAPSHOT_BLOCKER_MODES,
                "distraction blocker mode",
            )
        }
        _ => Err(AdapterError("distraction block rule snapshot")),
    })
}

fn validate_usage_bucket(value: &Value) -> Result<(), AdapterError> {
    let [_, source_type, _, Field::Text(date)] = value.fields() else {
        return Err(AdapterError("distraction usage bucket"));
    };
    one_of(
        Some(source_type),
        USAGE_SOURCE_TYPES,
        "distraction usage source type",
    )?;
    if !is_usage_date(date) {
        return Err(AdapterError("distraction usage date"));
    }
    Ok(())
}

/// Adapter of the distraction rules and usage domain.
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
        } else if table == USAGE_TOTALS_TABLE && group == usage_total_group::BUCKET {
            validate_usage_bucket(value)
        } else if table == BLOCK_EVENTS_TABLE && group == block_event_group::EVENT {
            validate_block_event(value)
        } else if table == BLOCK_EVENTS_TABLE && group == block_event_group::SNAPSHOT {
            validate_block_snapshot(value)
        } else {
            Ok(())
        }
    }

    /// A block event names a segment only together with its run: a segment of no published run
    /// would never arrive, so the reference would stay dangling.
    fn validate_row(&self, table: TableId, groups: &[GroupValue]) -> Result<(), AdapterError> {
        if table != BLOCK_EVENTS_TABLE {
            return Ok(());
        }
        let first_field = |group: GroupId| {
            groups
                .iter()
                .find(|value| value.group == group)
                .and_then(|value| value.value.fields().first())
        };
        match (
            first_field(block_event_group::RUN),
            first_field(block_event_group::SEGMENT),
        ) {
            (Some(Field::Null), Some(segment)) if *segment != Field::Null => {
                Err(AdapterError("distraction block segment without run"))
            }
            _ => Ok(()),
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
    /// Distraction rules derive nothing: the hook only reports changes.
    fn rebuild_all<'a>(
        &'a self,
        _conn: &'a mut SqliteConnection,
        _table: &'static TableSpec,
        _changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(std::future::ready(Ok(())))
    }

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

/// `hooks` with the rules hook on every rule table of the domain. Usage totals and block events
/// carry no rules, and their readers query them when they need them, so they report nothing.
pub fn with_hooks(hooks: Hooks) -> Hooks {
    DOMAIN
        .tables
        .iter()
        .filter(|table| table.id != USAGE_TOTALS_TABLE && table.id != BLOCK_EVENTS_TABLE)
        .fold(hooks, |hooks, table| {
            hooks.with(table.id, Box::new(DistractionsHook))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::OwnedRow;
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

        let bucket = |source_type: &str, date: &str| {
            adapter.validate(
                USAGE_TOTALS_TABLE,
                usage_total_group::BUCKET,
                &value(vec![
                    text("device-a"),
                    text(source_type),
                    text("video.example.com"),
                    text(date),
                ]),
            )
        };
        assert_eq!(bucket("website", "2026-10-23"), Ok(()));
        assert_eq!(bucket("mobile-app", "2026-02-01"), Ok(()));
        assert!(bucket("tablet-app", "2026-10-23").is_err());
        assert!(bucket("website", "2026-13-01").is_err());
        assert!(bucket("website", "2026-10-00").is_err());
        assert!(bucket("website", "2026/10/23").is_err());
        assert!(bucket("website", "26-10-2023").is_err());
        assert!(bucket("website", "2026-1a-23").is_err());

        let block = |source_type: &str, source_key: &str, phase: Field, decision: &str| {
            adapter.validate(
                BLOCK_EVENTS_TABLE,
                block_event_group::EVENT,
                &value(vec![
                    text("2027-01-05T09:10:00.000Z"),
                    text(source_type),
                    text(source_key),
                    Field::Null,
                    phase,
                    text(decision),
                    Field::Null,
                    Field::Null,
                    text("2027-01-05T09:10:00.000Z"),
                ]),
            )
        };
        assert_eq!(
            block("browser", "video.example.com", text("focus"), "blocked"),
            Ok(())
        );
        assert_eq!(
            block(
                "mobile_app",
                "com.example.video",
                Field::Null,
                "limit_exhausted"
            ),
            Ok(())
        );
        assert!(
            block(
                "browser",
                "https://video.example.com",
                Field::Null,
                "blocked"
            )
            .is_err()
        );
        assert!(block("tablet_app", "video.example.com", Field::Null, "blocked").is_err());
        assert!(block("browser", "video.example.com", text("lunch"), "blocked").is_err());
        assert!(block("browser", "video.example.com", Field::Null, "ignored").is_err());

        let snapshot = |kind: Field, mode: Field| {
            let row = OwnedRow::flat(vec![Field::Null, kind, Field::Null, Field::Null, mode]);
            let blob = codec::encode(&BLOCK_RULE_SNAPSHOTS, &[row]).unwrap();
            adapter.validate(
                BLOCK_EVENTS_TABLE,
                block_event_group::SNAPSHOT,
                &value(vec![Field::Blob(blob)]),
            )
        };
        assert_eq!(snapshot(text("domain"), text("blacklist")), Ok(()));
        assert_eq!(snapshot(Field::Null, Field::Null), Ok(()));
        assert!(snapshot(text("screen_content"), text("blacklist")).is_err());
        assert!(snapshot(text("domain"), text("graylist")).is_err());
    }

    #[test]
    fn a_block_event_names_a_segment_only_with_its_run() {
        let adapter = DistractionsAdapter;
        let row = |run: Field, segment: Field| {
            adapter.validate_row(
                BLOCK_EVENTS_TABLE,
                &[
                    GroupValue::new(BLOCK_EVENTS_TABLE, block_event_group::RUN, value(vec![run])),
                    GroupValue::new(
                        BLOCK_EVENTS_TABLE,
                        block_event_group::SEGMENT,
                        value(vec![segment]),
                    ),
                ],
            )
        };
        assert_eq!(row(text("run-1"), text("segment-1")), Ok(()));
        assert_eq!(row(text("run-1"), Field::Null), Ok(()));
        assert_eq!(row(Field::Null, Field::Null), Ok(()));
        assert!(row(Field::Null, text("segment-1")).is_err());
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

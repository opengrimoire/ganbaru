//! Restoring distraction rules offered for sync recovery.
//!
//! A deleted custom category or usage limit comes back as a new rule with a new id, written
//! through the same validation as a Settings edit so the restore seals like any local write. A
//! custom category returns with its retained hosts and takes a numeric name suffix when another
//! category uses its name; a usage limit returns with its retained budget and entries. Both keep
//! their retained enabled state.

use std::collections::HashSet;

use ganbaru_sync::RecoveryDescendant;
use ganbaru_sync::codec;
use ganbaru_sync::manifest::vault::distractions::{
    HOST_RULES_TABLE, LIMIT_ENTRIES, custom_category_group, host_group, limit_group,
};
use ganbaru_sync_contracts::{Field, GroupId, Value};
use sqlx::SqliteConnection;

use super::store::{self, LimitDraft, LimitEntryDraft, RuleTarget};
use super::{MAX_CATEGORY_NAME_CHARS, normalize_name};

/// Exclusive upper bound of the numeric suffix tried for a free category name.
const NAME_SUFFIX_CAP: u32 = 1000;

fn group_value(values: &[(GroupId, Value)], group: GroupId) -> Option<&[Field]> {
    values
        .iter()
        .find(|(id, _)| *id == group)
        .map(|(_, value)| value.fields())
}

fn text_field(field: Option<&Field>) -> Option<&str> {
    match field {
        Some(Field::Text(text)) => Some(text),
        _ => None,
    }
}

fn integer_field(field: Option<&Field>) -> Option<i64> {
    match field {
        Some(Field::Integer(value)) => Some(*value),
        _ => None,
    }
}

/// The retained enabled flag, or enabled when the group was not retained.
fn retained_enabled(values: &[(GroupId, Value)], group: GroupId) -> bool {
    group_value(values, group)
        .and_then(|fields| integer_field(fields.first()))
        .is_none_or(|flag| flag != 0)
}

/// `base`, or `base` with the lowest free numeric suffix from 2 when another category uses it,
/// ignoring case. `None` when no suffixed name fits the name bound.
async fn free_category_name(
    conn: &mut SqliteConnection,
    base: &str,
) -> Result<Option<String>, String> {
    let names: Vec<String> = sqlx::query_scalar("SELECT name FROM distractions_custom_categories")
        .fetch_all(&mut *conn)
        .await
        .map_err(|error| format!("read custom distraction category names: {error}"))?;
    let taken: HashSet<String> = names.iter().map(|name| name.to_lowercase()).collect();
    if !taken.contains(&base.to_lowercase()) {
        return Ok(Some(base.to_string()));
    }
    Ok((2..NAME_SUFFIX_CAP)
        .map(|suffix| format!("{base} {suffix}"))
        .find(|candidate| {
            candidate.chars().count() <= MAX_CATEGORY_NAME_CHARS
                && !taken.contains(&candidate.to_lowercase())
        }))
}

/// Restores a deleted custom category with the hosts deleted with it and returns its new id.
pub async fn restore_recovered_category(
    conn: &mut SqliteConnection,
    values: &[(GroupId, Value)],
    descendants: &[RecoveryDescendant],
    now_ms: i64,
) -> Result<String, String> {
    let name = group_value(values, custom_category_group::NAME)
        .and_then(|fields| text_field(fields.first()))
        .and_then(|name| normalize_name(name, MAX_CATEGORY_NAME_CHARS))
        .ok_or("a recovered custom category has no name")?;
    let name = free_category_name(conn, &name)
        .await?
        .ok_or("every name for the recovered custom category is taken")?;
    let mut hosts: Vec<(i64, String)> = descendants
        .iter()
        .filter(|row| row.table == HOST_RULES_TABLE)
        .filter_map(|row| {
            let host = text_field(group_value(&row.values, host_group::RULE)?.get(2))?;
            let created = group_value(&row.values, host_group::CREATED)
                .and_then(|fields| integer_field(fields.first()))
                .unwrap_or(i64::MAX);
            Some((created, host.to_string()))
        })
        .collect();
    hosts.sort();
    let hosts: Vec<String> = hosts.into_iter().map(|(_, host)| host).collect();
    let id = store::save_custom_category(conn, None, &name, &hosts, now_ms.max(1))
        .await
        .map_err(|error| format!("restore custom distraction category: {error}"))?;
    if !retained_enabled(values, custom_category_group::ENABLED) {
        let target = RuleTarget::CustomCategory { id: id.clone() };
        store::set_enabled(conn, &target, false)
            .await
            .map_err(|error| format!("restore custom distraction category state: {error}"))?;
    }
    Ok(id)
}

/// The editor's form of one retained limit entry row.
fn entry_draft(fields: &[Field]) -> Result<LimitEntryDraft, String> {
    let text = |index: usize| {
        text_field(fields.get(index))
            .unwrap_or_default()
            .to_string()
    };
    let match_names = text_field(fields.get(7)).unwrap_or("[]");
    Ok(LimitEntryDraft {
        id: text(0),
        name: text(1),
        color: integer_field(fields.get(2)),
        website_host: text(3),
        mobile_app_name: text(4),
        mobile_app_package: text(5),
        desktop_app_name: text(6),
        desktop_app_match_names: serde_json::from_str(match_names)
            .map_err(|error| format!("decode recovered usage limit match names: {error}"))?,
    })
}

/// Restores a deleted usage limit with its retained budget and entries and returns its new id.
pub async fn restore_recovered_limit(
    conn: &mut SqliteConnection,
    values: &[(GroupId, Value)],
    now_ms: i64,
) -> Result<String, String> {
    let name = group_value(values, limit_group::NAME)
        .and_then(|fields| text_field(fields.first()))
        .ok_or("a recovered usage limit has no name")?;
    let budget =
        group_value(values, limit_group::BUDGET).ok_or("a recovered usage limit has no budget")?;
    let entries = match group_value(values, limit_group::ENTRIES) {
        Some([Field::Blob(bytes)]) => codec::decode(&LIMIT_ENTRIES, bytes)
            .map_err(|reason| format!("decode recovered usage limit entries: {reason}"))?,
        _ => return Err("a recovered usage limit has no entries".to_string()),
    };
    let draft = LimitDraft {
        name: name.to_string(),
        minutes_per_day: integer_field(budget.first()),
        minutes_per_week: integer_field(budget.get(1)),
        entries: entries
            .iter()
            .map(|row| entry_draft(&row.fields))
            .collect::<Result<_, _>>()?,
    };
    let id = store::save_usage_limit(conn, None, &draft, now_ms.max(1))
        .await
        .map_err(|error| format!("restore usage limit: {error}"))?;
    if !retained_enabled(values, limit_group::ENABLED) {
        let target = RuleTarget::UsageLimit { id: id.clone() };
        store::set_enabled(conn, &target, false)
            .await
            .map_err(|error| format!("restore usage limit state: {error}"))?;
    }
    Ok(id)
}

//! Selection and exhausted-rule decisions over persisted native configuration.

use super::*;
use serde::Deserialize;

const MAX_DESKTOP_RULES: usize = 256;
const MAX_RULE_NAMES: usize = 64;

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DesktopRule {
    name: String,
    #[serde(default = "enabled")]
    enabled: bool,
    #[serde(default)]
    match_names: Vec<String>,
}
fn enabled() -> bool {
    true
}

/// Phase selection stays subordinate to the same final persisted-rule authorization.
pub(super) fn phase_rules(
    root: &Value,
    phase: Option<&DistractionsRuntimeState>,
) -> Result<Vec<DistractionsDesktopAppRuleInput>, String> {
    let Some(rules) = root.pointer("/distractions/desktop/blockedApps") else {
        return Ok(Vec::new());
    };
    let rules: Vec<DesktopRule> = serde_json::from_value(rules.clone())
        .map_err(|error| format!("invalid desktop rules: {error}"))?;
    if rules.len() > MAX_DESKTOP_RULES {
        return Err("desktop rule count exceeds its limit".into());
    }
    let mut result = Vec::new();
    for rule in rules {
        if rule.name.len() > 512
            || rule.match_names.len() > MAX_RULE_NAMES
            || rule.match_names.iter().any(|name| name.len() > 512)
        {
            return Err("desktop rule exceeds its text or name limit".into());
        }
        if !rule.enabled {
            continue;
        }
        let identity = DistractionsDesktopRuleIdentity::DesktopApp {
            rule_id: rule.name.clone(),
        };
        if configured_close_authorization(root, phase, None, &identity).is_ok() {
            result.push(DistractionsDesktopAppRuleInput {
                rule_identity: identity,
                name: rule.name,
                match_names: rule.match_names,
            });
        }
    }
    Ok(result)
}

fn is_safe_entry(entry: &limits::LimitEntry) -> bool {
    entry
        .desktop_app_name
        .as_deref()
        .is_some_and(|name| !is_protected_desktop_app_name(name))
        && !entry
            .desktop_app_match_names
            .iter()
            .any(|name| is_protected_desktop_app_name(name))
}

pub(super) fn usage_rules(config: &limits::LimitsConfig) -> Vec<DistractionsDesktopAppRuleInput> {
    if !config.enabled {
        return Vec::new();
    }
    config
        .items
        .iter()
        .filter(|limit| limit.enabled)
        .flat_map(|limit| {
            limit
                .entries
                .iter()
                .filter(|entry| is_safe_entry(entry))
                .map(|entry| DistractionsDesktopAppRuleInput {
                    name: entry.desktop_app_name.clone().unwrap_or_default(),
                    match_names: entry.desktop_app_match_names.clone(),
                    rule_identity: DistractionsDesktopRuleIdentity::UsageLimit {
                        rule_id: limit.id.clone(),
                        entry_id: entry.id.clone(),
                    },
                })
        })
        .collect()
}

/// Match configured identities only. Never use a window title or collect unselected applications.
pub(super) fn foreground_source(
    config: &limits::LimitsConfig,
    status: &DistractionsForegroundDesktopAppStatus,
) -> Option<accounting::UsageSource> {
    if !status.available || !config.enabled {
        return None;
    }
    let observed = foreground_status_match_names(status);
    if observed
        .iter()
        .any(|name| is_protected_desktop_app_name(name))
    {
        return None;
    }
    for limit in &config.items {
        if !limit.enabled {
            continue;
        }
        for entry in &limit.entries {
            if !is_safe_entry(entry) {
                continue;
            }
            let configured = if entry.desktop_app_match_names.is_empty() {
                vec![entry.desktop_app_name.clone()?]
            } else {
                entry.desktop_app_match_names.clone()
            };
            if let Some(name) = configured.into_iter().find(|name| {
                observed
                    .iter()
                    .any(|observed| app_name_key(observed) == app_name_key(name))
            }) {
                return Some(accounting::UsageSource {
                    key: app_name_key(&name),
                    label: entry.desktop_app_name.clone()?,
                });
            }
        }
    }
    None
}

pub(super) fn open_sources(
    matches: &[DistractionsRunningDesktopAppMatch],
) -> Vec<accounting::UsageSource> {
    let mut seen = HashSet::new();
    matches
        .iter()
        .filter_map(|observed| {
            if is_protected_desktop_app_name(&observed.app_name)
                || is_protected_desktop_app_name(&observed.process_name)
            {
                return None;
            }
            let key = app_name_key(&observed.process_name);
            // Process-open mode counts one selected application once, even with several processes.
            if !seen.insert(app_name_key(&observed.app_name)) {
                return None;
            }
            Some(accounting::UsageSource {
                key,
                label: observed.app_name.clone(),
            })
        })
        .collect()
}

pub(super) fn exhausted_rule(
    config: &limits::LimitsConfig,
    totals: &[limits::BudgetTotal],
    source: &accounting::UsageSource,
) -> Option<(DistractionsDesktopRuleIdentity, String)> {
    if !config.enabled {
        return None;
    }
    let sample = limits::UsageSourceDay {
        source_type: "desktop-app".into(),
        source_key: source.key.clone(),
        local_date: String::new(),
        elapsed_seconds: 0,
    };
    for limit in &config.items {
        if !limit.enabled
            || !totals.iter().any(|total| {
                total.limit_id == limit.id
                    && total.exhausted
                    && total.used_seconds >= total.limit_seconds
            })
        {
            continue;
        }
        if let Some(entry) = limit
            .entries
            .iter()
            .find(|entry| is_safe_entry(entry) && limits::matches(entry, &sample))
        {
            return Some((
                DistractionsDesktopRuleIdentity::UsageLimit {
                    rule_id: limit.id.clone(),
                    entry_id: entry.id.clone(),
                },
                limit.name.clone(),
            ));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn foreground_accounting_requires_an_enabled_selected_identity_and_rejects_protected_aliases() {
        let root = serde_json::json!({"distractions": {"limits": {"items": [{"id": "games", "minutesPerDay": 10,
            "entries": [{"id": "game", "desktopAppName": "My game", "desktopAppMatchNames": ["game.exe"]}]}]}}});
        let config = limits::parse_config(&root).unwrap();
        let mut status = foreground_status_from_parts(
            "Game title",
            Some("game.exe".into()),
            Some(42),
            vec!["game.exe".into()],
        );
        let selected = foreground_source(&config, &status).unwrap();
        assert_eq!(selected.key, "game.exe");
        assert_eq!(selected.label, "My game");
        status.match_names.push("bash".into());
        assert!(foreground_source(&config, &status).is_none());
        status = foreground_status_from_parts(
            "Unselected app",
            Some("other.exe".into()),
            Some(43),
            vec![],
        );
        assert!(foreground_source(&config, &status).is_none());
        let mut disabled = config;
        disabled.enabled = false;
        assert!(foreground_source(&disabled, &status).is_none());
    }

    #[test]
    fn process_open_sources_deduplicate_application_processes_and_exhaustion_selects_exact_entry() {
        let root = serde_json::json!({"distractions": {"limits": {"items": [{"id": "games", "name": "Games", "minutesPerDay": 1,
            "entries": [{"id": "game", "desktopAppName": "Game", "desktopAppMatchNames": ["game"]}]}]}}});
        let config = limits::parse_config(&root).unwrap();
        let observed = DistractionsRunningDesktopAppMatch {
            app_name: "Game".into(),
            process_name: "game".into(),
            process_id: 42,
            process_identity: "exact-process".into(),
            rule_identity: usage_rules(&config)[0].rule_identity.clone(),
        };
        let mut another = observed.clone();
        another.process_id = 43;
        let sources = open_sources(&[observed, another]);
        assert_eq!(sources.len(), 1);
        let totals = limits::totals(
            &config,
            &[limits::UsageSourceDay {
                source_type: "desktop-app".into(),
                source_key: "game".into(),
                local_date: "2026-10-02".into(),
                elapsed_seconds: 60,
            }],
            "2026-10-02",
        )
        .unwrap();
        assert_eq!(
            exhausted_rule(&config, &totals, &sources[0]).unwrap().0,
            DistractionsDesktopRuleIdentity::UsageLimit {
                rule_id: "games".into(),
                entry_id: "game".into()
            }
        );
        assert!(exhausted_rule(&config, &[], &sources[0]).is_none());
    }
}

use super::catalog::{DesktopCatalogIndex, desktop_catalog_index};
use super::*;

#[derive(Clone, Debug)]
pub(super) struct DesktopCloseAuthorization {
    match_names: Vec<String>,
    match_name_keys: HashSet<String>,
}

impl DesktopCloseAuthorization {
    /// Match names the persisted rule and the installed app catalog both vouch for.
    pub(super) fn match_names(&self) -> &[String] {
        &self.match_names
    }
}

fn json_string_list(value: Option<&Value>) -> Vec<String> {
    value
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Protected aliases are already removed by normalization, so an empty result means the rule
/// names nothing this device may close.
fn trusted_rule_names(
    name: &str,
    match_names: Vec<String>,
    catalog: &DesktopCatalogIndex,
) -> Option<Vec<String>> {
    let names = trusted_process_match_names(name, match_names, catalog.process_names(name));
    (!names.is_empty()).then_some(names)
}

fn json_enabled(value: &Value) -> bool {
    value
        .get("enabled")
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

fn configured_desktop_process_names(
    rule: &Value,
    catalog: &DesktopCatalogIndex,
) -> Result<Vec<String>, String> {
    let name = rule
        .get("name")
        .and_then(Value::as_str)
        .and_then(normalize_app_candidate_name)
        .ok_or_else(|| "configured desktop rule name is invalid".to_string())?;
    trusted_rule_names(&name, json_string_list(rule.get("matchNames")), catalog)
        .ok_or_else(|| "configured desktop rule is not closeable".to_string())
}

fn desktop_runtime_authorizes_close(desktop: &Value, runtime: &DistractionsRuntimeState) -> bool {
    if !runtime.active {
        return false;
    }
    let strict_pause =
        runtime.paused && matches!(runtime.pause_reason.as_deref(), Some("idle" | "suspend"));
    if runtime.paused
        && !strict_pause
        && desktop
            .get("pauseDuringFocusPause")
            .and_then(Value::as_bool)
            .unwrap_or(true)
    {
        return false;
    }
    let setting = match runtime.phase.as_str() {
        "focus" => "blockDuringFocus",
        "short_break" => "blockDuringShortBreaks",
        "long_break" => "blockDuringLongBreaks",
        _ => return false,
    };
    desktop
        .get(setting)
        .and_then(Value::as_bool)
        .unwrap_or(true)
}

pub(super) fn configured_close_authorization(
    config: &Value,
    runtime: Option<&DistractionsRuntimeState>,
    limit_state: Option<&DistractionsLimitState>,
    rule_identity: &DistractionsDesktopRuleIdentity,
    catalog: &DesktopCatalogIndex,
) -> Result<DesktopCloseAuthorization, String> {
    let distractions = config
        .get("distractions")
        .ok_or_else(|| "persisted distraction configuration is unavailable".to_string())?;
    let names = match rule_identity {
        DistractionsDesktopRuleIdentity::DesktopApp { rule_id } => {
            let rule_id = normalize_app_candidate_name(rule_id)
                .ok_or_else(|| "desktop rule identity is invalid".to_string())?;
            let desktop = distractions.get("desktop").ok_or_else(|| {
                "persisted desktop blocker configuration is unavailable".to_string()
            })?;
            if !json_enabled(desktop) {
                return Err("desktop blocker is disabled".to_string());
            }
            let runtime = runtime.ok_or_else(|| {
                "desktop blocker runtime authorization is unavailable".to_string()
            })?;
            if !desktop_runtime_authorizes_close(desktop, runtime) {
                return Err("desktop blocker is not active for the current phase".to_string());
            }
            let rule = desktop
                .get("blockedApps")
                .and_then(Value::as_array)
                .and_then(|rules| {
                    rules.iter().find(|rule| {
                        rule.get("name")
                            .and_then(Value::as_str)
                            .and_then(normalize_app_candidate_name)
                            .is_some_and(|name| app_name_key(&name) == app_name_key(&rule_id))
                    })
                })
                .ok_or_else(|| "desktop blocker rule is no longer configured".to_string())?;
            if !json_enabled(rule) {
                return Err("desktop blocker rule is disabled".to_string());
            }
            configured_desktop_process_names(rule, catalog)?
        }
        DistractionsDesktopRuleIdentity::UsageLimit { rule_id, entry_id } => {
            let native_config = limits::parse_config(config)?;
            let current_limit = native_config
                .items
                .iter()
                .find(|limit| limit.id == *rule_id)
                .ok_or_else(|| "usage limit rule is no longer configured".to_string())?;
            if limit_state.and_then(|state| state.configuration_digest.as_deref())
                != Some(limits::configuration_digest(config)?.as_str())
            {
                return Err("usage limit configuration changed after accounting".into());
            }
            let limits = distractions
                .get("limits")
                .ok_or_else(|| "persisted usage limit configuration is unavailable".to_string())?;
            if !json_enabled(limits) {
                return Err("usage limits are disabled".to_string());
            }
            let limit = limits
                .get("items")
                .and_then(Value::as_array)
                .and_then(|limits| {
                    limits.iter().find(|limit| {
                        limit.get("id").and_then(Value::as_str) == Some(rule_id.as_str())
                    })
                })
                .ok_or_else(|| "usage limit rule is no longer configured".to_string())?;
            if !json_enabled(limit) {
                return Err("usage limit rule is disabled".to_string());
            }
            let entry = limit
                .get("entries")
                .and_then(Value::as_array)
                .and_then(|entries| {
                    entries.iter().find(|entry| {
                        entry.get("id").and_then(Value::as_str) == Some(entry_id.as_str())
                    })
                })
                .ok_or_else(|| "usage limit desktop entry is no longer configured".to_string())?;
            let app_name = entry
                .get("desktopAppName")
                .and_then(Value::as_str)
                .and_then(normalize_app_candidate_name)
                .ok_or_else(|| "usage limit desktop entry is invalid".to_string())?;
            let names = trusted_rule_names(
                &app_name,
                json_string_list(entry.get("desktopAppMatchNames")),
                catalog,
            )
            .ok_or_else(|| "usage limit desktop entry is not closeable".to_string())?;
            if !limit_state.is_some_and(|state| {
                state.limits.iter().any(|limit| {
                    let minutes = match limit.period.as_str() {
                        "day" => current_limit.minutes_per_day,
                        "week" => current_limit.minutes_per_week,
                        _ => None,
                    };
                    limit.id == *rule_id
                        && limit.exhausted
                        && limit.used_seconds >= limit.limit_seconds
                        && minutes.is_some_and(|minutes| minutes * 60 == limit.limit_seconds)
                        && state_files::validate_limit_state(state).is_ok()
                })
            }) {
                return Err("usage limit is not currently exhausted".to_string());
            }
            names
        }
    };
    Ok(DesktopCloseAuthorization {
        match_name_keys: names.iter().map(|name| app_name_key(name)).collect(),
        match_names: names,
    })
}

pub(super) fn validate_names_authorized(
    names: impl IntoIterator<Item = String>,
    authorization: &DesktopCloseAuthorization,
) -> Result<(), String> {
    if names
        .into_iter()
        .any(|name| authorization.match_name_keys.contains(&app_name_key(&name)))
    {
        Ok(())
    } else {
        Err("current process identity is not authorized by the persisted rule".to_string())
    }
}

/// Authorizes closing apps for `rule_identity` under `config`, the rules snapshot the caller
/// observed with and has checked is still current.
pub(super) fn load_close_authorization<R: Runtime>(
    app: &tauri::AppHandle<R>,
    config: &Value,
    rule_identity: &DistractionsDesktopRuleIdentity,
) -> Result<DesktopCloseAuthorization, String> {
    let checked_at = now_utc();
    let runtime = matches!(
        rule_identity,
        DistractionsDesktopRuleIdentity::DesktopApp { .. }
    )
    .then(|| {
        crate::pomodoro::native_runtime::current_effect(app, checked_at.timestamp_millis())?
            .map(|effect| {
                state_files::committed_focus_state(&effect, checked_at.timestamp_millis())
            })
            .transpose()
    })
    .transpose()?
    .flatten();
    let limit_state = matches!(
        rule_identity,
        DistractionsDesktopRuleIdentity::UsageLimit { .. }
    )
    .then(|| {
        read_fresh_limit_state(
            &limit_state_path(app).ok()?,
            checked_at,
            &vault::active_database_path(app).ok()?,
        )
    })
    .flatten();
    if let Some(state) = &limit_state {
        let zone = ganbaru_civil_time::system_zone()?;
        let current_date =
            ganbaru_civil_time::instant_to_local(checked_at.timestamp_millis(), &zone)?
                .date()
                .format("%Y-%m-%d")
                .to_string();
        if state.local_date != current_date {
            return Err("usage limit snapshot belongs to an earlier local day".into());
        }
    }
    configured_close_authorization(
        config,
        runtime.as_ref(),
        limit_state.as_ref(),
        rule_identity,
        &desktop_catalog_index(),
    )
}

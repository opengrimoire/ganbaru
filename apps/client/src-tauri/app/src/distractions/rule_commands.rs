//! Settings commands that read and edit the vault's distraction rules, one rule change each.

use super::rule_store;
use ganbaru_distractions::config::store::{
    self, HostList, LimitDraft, RuleEdit, RuleRefusal, RuleTarget, RulesError,
};
use ganbaru_distractions::config::{DistractionsConfig, DistractionsMode};
use serde::Serialize;
use tauri::{AppHandle, Runtime};

/// Result of a rule edit: the rules after it, or why it was refused without a change.
#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum RulesEditOutcome {
    Saved { config: Box<DistractionsConfig> },
    Refused { reason: RuleRefusal },
}

fn now_epoch_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Applies `edit` and publishes the change to the blockers when it was saved.
async fn edit<R: Runtime>(app: &AppHandle<R>, edit: RuleEdit) -> Result<RulesEditOutcome, String> {
    let pool = crate::db::connect_vault(app).await?;
    match store::apply(&pool, &edit, now_epoch_ms()).await {
        Ok(config) => {
            rule_store::after_rules_changed(app);
            Ok(RulesEditOutcome::Saved {
                config: Box::new(config),
            })
        }
        Err(RulesError::Refused(reason)) => Ok(RulesEditOutcome::Refused { reason }),
        Err(error @ RulesError::Storage(_)) => Err(error.to_string()),
    }
}

/// Reads the rules of the active vault.
#[tauri::command]
pub async fn distractions_rules_load<R: Runtime>(
    app: AppHandle<R>,
) -> Result<DistractionsConfig, String> {
    let pool = crate::db::connect_vault(&app).await?;
    store::read(&pool).await.map_err(|error| error.to_string())
}

/// Switches between blocking listed sites and allowing only listed sites.
#[tauri::command]
pub async fn distractions_rules_set_mode<R: Runtime>(
    app: AppHandle<R>,
    mode: DistractionsMode,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::SetMode(mode)).await
}

/// Turns one setting or rule on or off.
#[tauri::command]
pub async fn distractions_rules_set_enabled<R: Runtime>(
    app: AppHandle<R>,
    target: RuleTarget,
    enabled: bool,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::SetEnabled { target, enabled }).await
}

/// Adds hosts to the blocked, exception, or allowed list.
#[tauri::command]
pub async fn distractions_rules_add_hosts<R: Runtime>(
    app: AppHandle<R>,
    list: HostList,
    hosts: Vec<String>,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::AddHosts { list, hosts }).await
}

/// Blocks a desktop app by name and process match names.
#[tauri::command]
pub async fn distractions_rules_add_desktop_app<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    match_names: Vec<String>,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::AddDesktopApp { name, match_names }).await
}

/// Blocks an Android app by package name.
#[tauri::command]
pub async fn distractions_rules_add_mobile_app<R: Runtime>(
    app: AppHandle<R>,
    name: String,
    package_name: String,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::AddMobileApp { name, package_name }).await
}

/// Removes a custom category, host, app, or usage limit.
#[tauri::command]
pub async fn distractions_rules_remove<R: Runtime>(
    app: AppHandle<R>,
    target: RuleTarget,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::Remove(target)).await
}

/// Creates a custom category, or renames one and replaces its hosts.
#[tauri::command]
pub async fn distractions_rules_save_custom_category<R: Runtime>(
    app: AppHandle<R>,
    id: Option<String>,
    name: String,
    hosts: Vec<String>,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::SaveCustomCategory { id, name, hosts }).await
}

/// Creates a usage limit, or replaces the name, budget, and ordered entries of one.
#[tauri::command]
pub async fn distractions_rules_save_usage_limit<R: Runtime>(
    app: AppHandle<R>,
    id: Option<String>,
    draft: LimitDraft,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::SaveUsageLimit { id, draft }).await
}

/// Restores the default rules.
#[tauri::command]
pub async fn distractions_rules_reset<R: Runtime>(
    app: AppHandle<R>,
) -> Result<RulesEditOutcome, String> {
    edit(&app, RuleEdit::Reset).await
}

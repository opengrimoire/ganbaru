//! The vault's distraction rules as the blockers read them, and the publication of rule changes.
//!
//! Rules live in SQLite. Blockers read one assembled snapshot, keep it while they observe and
//! enforce, and check its revision before acting, so an edit from Settings or a sync apply
//! revokes decisions made from older rules.

use crate::vault;
use ganbaru_distractions::config::store;
use ganbaru_sync::domains::distractions::RULES_TOPIC;
use ganbaru_sync::manifest::vault::distractions::{BLOCK_EVENTS_TABLE, DOMAIN, USAGE_TOTALS_TABLE};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Runtime};

/// Counts rule changes in this process. Readers capture it before reading the rules.
static REVISION: AtomicU64 = AtomicU64::new(0);

/// Assembled rules of one vault at one revision.
pub(crate) struct RulesSnapshot {
    /// Rule revision captured before the read.
    pub(crate) revision: u64,
    /// Vault the rules were read from.
    pub(crate) vault_id: String,
    /// The rules under a `distractions` key, the shape policy and budget readers take.
    pub(crate) root: Value,
}

/// Reads the active vault's rules. The vault must stay active through the read.
pub(crate) async fn snapshot<R: Runtime>(app: &AppHandle<R>) -> Result<RulesSnapshot, String> {
    let revision = REVISION.load(Ordering::Acquire);
    let vault_id = vault::active_vault_id(app)?;
    let pool = crate::db::connect_vault(app).await?;
    let config = store::read(&pool)
        .await
        .map_err(|error| error.to_string())?;
    if vault::active_vault_id(app)? != vault_id {
        return Err("vault changed while reading distraction rules".into());
    }
    Ok(RulesSnapshot {
        revision,
        vault_id,
        root: config.to_root_value()?,
    })
}

/// Whether no rule change was published since `revision` was captured.
pub(crate) fn is_current(revision: u64) -> bool {
    REVISION.load(Ordering::Acquire) == revision
}

/// Publishes a committed rule change: revokes snapshots read before it and wakes the blockers,
/// which reread the rules and republish what browser, desktop, and Android enforcement read.
pub(crate) fn after_rules_changed<R: Runtime>(app: &AppHandle<R>) {
    REVISION.fetch_add(1, Ordering::AcqRel);
    #[cfg(desktop)]
    if let Err(error) = super::runtime::commit_configuration(app, || Ok(())) {
        eprintln!("publish distraction rule change: {error}");
    }
    #[cfg(target_os = "android")]
    if let Err(error) = super::android::runtime::commit_configuration(app, || Ok(())) {
        // The publisher's own revision check revokes Guardian rules on its next pass.
        eprintln!("publish distraction rule change: {error}");
        super::android::runtime::wake(app);
    }
    #[cfg(not(any(desktop, target_os = "android")))]
    let _ = app;
}

/// Publishes rules a sync pass changed, by apply or recovery restore, through the same path
/// local edits take. `changed` holds the table names and notice topics the pass reported.
pub(crate) fn on_sync_changes<R: Runtime>(app: &AppHandle<R>, changed: &[String]) {
    if rules_changed(changed) {
        after_rules_changed(app);
    }
}

/// Whether `changed` names a rule table, an owned table of one, or the rules notice topic.
/// Usage totals and block events carry no rules, so applying them republishes nothing.
fn rules_changed(changed: &[String]) -> bool {
    changed
        .iter()
        .any(|name| name == RULES_TOPIC || is_rule_table(name))
}

fn is_rule_table(name: &str) -> bool {
    DOMAIN
        .tables
        .iter()
        .filter(|table| table.id != USAGE_TOTALS_TABLE && table.id != BLOCK_EVENTS_TABLE)
        .any(|table| {
            table.name == name || table.owned_children().any(|(_, owned)| owned.table == name)
        })
}

#[cfg(test)]
mod tests {
    use super::rules_changed;
    use ganbaru_sync::domains::distractions::RULES_TOPIC;
    use ganbaru_sync::manifest::vault::distractions::{
        APP_RULES, BLOCK_EVENTS, BLOCK_RULE_SNAPSHOTS_OWNED, CATEGORY_RULES, CUSTOM_CATEGORIES,
        HOST_RULES, LIMIT_ENTRIES_OWNED, SETTINGS, USAGE_LIMITS, USAGE_SAMPLES_LOCAL, USAGE_TOTALS,
    };

    fn names(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn a_remote_rule_apply_or_restore_republishes_the_rules() {
        for table in [
            SETTINGS.name,
            CATEGORY_RULES.name,
            CUSTOM_CATEGORIES.name,
            HOST_RULES.name,
            APP_RULES.name,
            USAGE_LIMITS.name,
            LIMIT_ENTRIES_OWNED,
        ] {
            assert!(rules_changed(&names(&[table])), "{table} holds rules");
        }
        assert!(rules_changed(&names(&[USAGE_TOTALS.name, RULES_TOPIC])));
    }

    #[test]
    fn applied_usage_totals_and_block_history_do_not_republish_the_rules() {
        assert!(!rules_changed(&names(&[
            USAGE_TOTALS.name,
            BLOCK_EVENTS.name,
            BLOCK_RULE_SNAPSHOTS_OWNED,
            USAGE_SAMPLES_LOCAL,
            "calendar_events",
        ])));
        assert!(!rules_changed(&[]));
    }
}

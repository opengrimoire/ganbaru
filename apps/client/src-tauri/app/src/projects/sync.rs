//! Projects work that follows rows sync applied.
//!
//! The Projects post-apply hook reports each changed row under its table name. The host hands
//! those reports to [`on_sync_notices`], which publishes the window change event Projects views
//! already listen to, so open windows reload what they show and the task reminder schedule
//! reloads. Applied rows can be deleted ones that no longer name their project, so the event
//! always carries [`SYNC_CHANGE_KEY`], which tells views to drop every cached project.

use ganbaru_sync::ChangeNotice;
use ganbaru_sync::manifest::vault::projects;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// Window event Projects views listen to for changes made elsewhere.
pub(crate) const PROJECTS_CHANGED_EVENT: &str = "projects-changed";

/// Change key meaning that rows of any project may have changed. It matches
/// `PROJECTS_SYNC_CHANGE_KEY` in the frontend Projects window sync module.
pub(crate) const SYNC_CHANGE_KEY: &str = "sync";

/// Source of the change event; it differs from every window's random source id.
const SYNC_SOURCE_ID: &str = "sync";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultChange<'a> {
    vault_id: &'a str,
    keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangeEnvelope<'a> {
    source_id: &'a str,
    payload: VaultChange<'a>,
}

/// Change keys for the Projects rows among `notices`: [`SYNC_CHANGE_KEY`] first, then the
/// distinct changed project ids. `None` when no Projects row changed.
fn change_keys<'a>(notices: impl Iterator<Item = &'a ChangeNotice>) -> Option<Vec<String>> {
    let mut changed = false;
    let mut project_ids = std::collections::BTreeSet::new();
    for notice in notices {
        if !projects::DOMAIN
            .tables
            .iter()
            .any(|table| table.name == notice.topic)
        {
            continue;
        }
        changed = true;
        if notice.topic == projects::PROJECTS.name {
            project_ids.insert(notice.key.clone());
        }
    }
    changed.then(|| {
        std::iter::once(SYNC_CHANGE_KEY.to_string())
            .chain(project_ids)
            .collect()
    })
}

/// Publishes the Projects change event when sync change `notices` include Projects rows.
pub(crate) fn on_sync_notices<R: Runtime>(app: &AppHandle<R>, notices: &[ChangeNotice]) {
    let Some(keys) = change_keys(notices.iter()) else {
        return;
    };
    let vault_id = match crate::vault::active_vault_id(app) {
        Ok(vault_id) => vault_id,
        Err(error) => {
            eprintln!("publish projects sync changes: {error}");
            return;
        }
    };
    let envelope = ChangeEnvelope {
        source_id: SYNC_SOURCE_ID,
        payload: VaultChange {
            vault_id: &vault_id,
            keys,
        },
    };
    if let Err(error) = app.emit(PROJECTS_CHANGED_EVENT, envelope) {
        eprintln!("publish projects sync changes: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(topic: &'static str, key: &str) -> ChangeNotice {
        ChangeNotice {
            topic,
            key: key.to_string(),
        }
    }

    #[test]
    fn projects_sync_change_keys_name_changed_projects_after_the_sync_key() {
        let notices = [
            notice(projects::TASKS.name, "task-a"),
            notice(projects::PROJECTS.name, "project-b"),
            notice("vault_preferences", "project-c"),
            notice(projects::PROJECTS.name, "project-a"),
            notice(projects::PROJECTS.name, "project-b"),
        ];
        assert_eq!(
            change_keys(notices.iter()),
            Some(vec![
                SYNC_CHANGE_KEY.to_string(),
                "project-a".to_string(),
                "project-b".to_string(),
            ])
        );
    }

    #[test]
    fn projects_sync_change_keys_cover_a_deleted_child_row_without_its_project() {
        let notices = [notice(projects::CHECKLIST.name, "checklist-a")];
        assert_eq!(
            change_keys(notices.iter()),
            Some(vec![SYNC_CHANGE_KEY.to_string()])
        );
    }

    #[test]
    fn projects_sync_change_keys_ignore_other_domains() {
        let notices = [notice("vault_preferences", "projects.view")];
        assert_eq!(change_keys(notices.iter()), None);
    }

    #[test]
    fn projects_sync_key_prefixes_match_the_projects_crate() {
        let manifest = [
            projects::PRIORITY_KEY_PREFIX,
            projects::DEPENDENCY_KEY_PREFIX,
            projects::TASK_TAG_KEY_PREFIX,
            projects::CUSTOM_FIELD_VALUE_KEY_PREFIX,
            projects::VIEW_PREFERENCE_KEY_PREFIX,
        ];
        let crate_keys = [
            ganbaru_projects::keys::PRIORITY_KEY_PREFIX,
            ganbaru_projects::keys::DEPENDENCY_KEY_PREFIX,
            ganbaru_projects::keys::TASK_TAG_KEY_PREFIX,
            ganbaru_projects::keys::CUSTOM_FIELD_VALUE_KEY_PREFIX,
            ganbaru_projects::keys::VIEW_PREFERENCE_KEY_PREFIX,
        ];
        assert_eq!(manifest, crate_keys);
    }
}

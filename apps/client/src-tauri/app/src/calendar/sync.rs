//! Calendar work that follows rows sync applied.
//!
//! The Calendar post-apply hook (`ganbaru_calendar::sync_apply`) rebuilds component projections
//! inside the engine transaction and reports each changed Calendar row under its table name. The
//! host hands those reports to [`on_sync_notices`], which publishes the Calendar window change
//! event every Calendar store already listens to, so open windows reload and desktop
//! notification schedules follow the reloaded events, and marks the native Focus Calendar
//! projection stale so titles, links, and admission read the applied rows.

use ganbaru_sync::ChangeNotice;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Runtime};

/// Window event Calendar stores listen to for changes made elsewhere. It matches
/// `CALENDAR_WINDOW_SYNC_EVENT` in the frontend Calendar window sync module.
pub(crate) const CALENDAR_WINDOW_SYNC_EVENT: &str = "calendar-window-sync";

/// Source of the change event; it differs from every window's random source id.
const SYNC_SOURCE_ID: &str = "sync";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DataChanged {
    kind: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ChangeEnvelope {
    source_id: &'static str,
    payload: DataChanged,
}

/// Whether sync change `notices` include Calendar rows.
fn calendar_changed(notices: &[ChangeNotice]) -> bool {
    notices
        .iter()
        .any(|notice| ganbaru_calendar::sync_apply::is_calendar_topic(notice.topic))
}

/// Refreshes Calendar views and the Focus Calendar projection when sync change `notices`
/// include Calendar rows.
pub(crate) fn on_sync_notices<R: Runtime>(app: &AppHandle<R>, notices: &[ChangeNotice]) {
    if !calendar_changed(notices) {
        return;
    }
    crate::pomodoro::invalidate_calendar(app);
    let envelope = ChangeEnvelope {
        source_id: SYNC_SOURCE_ID,
        payload: DataChanged {
            kind: "data-changed",
        },
    };
    if let Err(error) = app.emit(CALENDAR_WINDOW_SYNC_EVENT, envelope) {
        eprintln!("publish calendar sync changes: {error}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ganbaru_sync::manifest::vault::calendar;

    fn notice(topic: &'static str) -> ChangeNotice {
        ChangeNotice {
            topic,
            key: "row".to_string(),
        }
    }

    #[test]
    fn calendar_sync_refreshes_for_any_calendar_table_and_ignores_other_domains() {
        for table in calendar::DOMAIN.tables {
            assert!(calendar_changed(&[notice(table.name)]), "{}", table.name);
        }
        assert!(!calendar_changed(&[notice("project_tasks")]));
        assert!(!calendar_changed(&[]));
    }

    #[test]
    fn calendar_sync_envelope_matches_the_frontend_window_sync_shape() {
        let value = serde_json::to_value(ChangeEnvelope {
            source_id: SYNC_SOURCE_ID,
            payload: DataChanged {
                kind: "data-changed",
            },
        })
        .unwrap();
        assert_eq!(
            value,
            serde_json::json!({ "sourceId": "sync", "payload": { "kind": "data-changed" } })
        );
    }
}

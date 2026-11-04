//! Focus work that follows rows sync applied.
//!
//! The Focus post-apply hook reports each changed Focus table under the Focus topic. Applied
//! closed runs change the history Calendar views render, and applied device activity changes
//! which occurrences automatic admission may start, so the host marks the native Focus Calendar
//! projection stale. Focus panels and Calendar stores follow the applied notice the frontend
//! receives for the topic.

use ganbaru_sync::ChangeNotice;
use ganbaru_sync::domains::focus::TOPIC;
use tauri::{AppHandle, Runtime};

/// Whether sync change `notices` include Focus rows.
fn focus_changed(notices: &[ChangeNotice]) -> bool {
    notices.iter().any(|notice| notice.topic == TOPIC)
}

/// Marks the native Focus Calendar projection stale when sync change `notices` include Focus
/// rows.
pub(crate) fn on_sync_notices<R: Runtime>(app: &AppHandle<R>, notices: &[ChangeNotice]) {
    if focus_changed(notices) {
        crate::pomodoro::invalidate_calendar(app);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn notice(topic: &'static str) -> ChangeNotice {
        ChangeNotice {
            topic,
            key: "row".to_string(),
        }
    }

    #[test]
    fn focus_sync_refreshes_only_for_focus_notices() {
        assert!(focus_changed(&[notice(TOPIC)]));
        assert!(focus_changed(&[notice("calendar_events"), notice(TOPIC)]));
        assert!(!focus_changed(&[notice("calendar_events")]));
        assert!(!focus_changed(&[]));
    }
}

//! Changes background passes applied while app views may still be loaded.
//!
//! A background pass runs in the app process with no app handle, and a webview in the background
//! stays loaded across it, so views cannot hear about the rows it applied. The pass records its
//! changes here, and the app reports them when it returns to the foreground. A process that died
//! takes the record with it, which loses nothing because a new app run reads every row.

use ganbaru_sync::ChangeNotice;
use std::collections::BTreeSet;
use std::sync::{Mutex, MutexGuard, PoisonError};

static PENDING: Mutex<BackgroundChanges> = Mutex::new(BackgroundChanges::new());

/// Distinct tables and notices that background passes changed.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct BackgroundChanges {
    pub(super) tables: BTreeSet<String>,
    pub(super) notices: BTreeSet<ChangeNotice>,
}

impl BackgroundChanges {
    const fn new() -> Self {
        Self {
            tables: BTreeSet::new(),
            notices: BTreeSet::new(),
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.tables.is_empty() && self.notices.is_empty()
    }
}

/// The record stays valid after a panic, since each update is a plain set insertion.
fn pending() -> MutexGuard<'static, BackgroundChanges> {
    PENDING.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Records the tables a background pass changed.
pub(super) fn record_tables(tables: Vec<String>) {
    pending().tables.extend(tables);
}

/// Records the notices a background pass reported.
pub(super) fn record_notices(notices: &[ChangeNotice]) {
    pending().notices.extend(notices.iter().cloned());
}

/// Takes everything recorded since the last call.
pub(super) fn take() -> BackgroundChanges {
    std::mem::replace(&mut *pending(), BackgroundChanges::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_changes_merge_across_passes_and_clear_when_taken() {
        take();
        let notice = ChangeNotice {
            topic: "calendar_events",
            key: "event-a".to_string(),
        };

        record_tables(vec!["calendar_events".to_string()]);
        record_notices(std::slice::from_ref(&notice));
        record_tables(vec!["calendar_events".to_string(), "focus".to_string()]);
        record_notices(std::slice::from_ref(&notice));

        let taken = take();
        assert_eq!(
            taken.tables,
            BTreeSet::from(["calendar_events".to_string(), "focus".to_string()])
        );
        assert_eq!(taken.notices, BTreeSet::from([notice]));
        assert!(take().is_empty());
    }
}

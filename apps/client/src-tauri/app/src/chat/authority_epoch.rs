//! Device-local counter of Chat authority changes.
//!
//! Every path on this device that can narrow what a running organizational turn may do bumps the
//! counter after it commits: access revocation drains, local deletes that cascade into Chat
//! authorizations, project updates that may archive a project, run launch settlement, and every
//! sync apply that touches Chat or Projects rows. A live authorization check that passed at one
//! epoch is reused only while the epoch is unchanged.

use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Manager, Runtime};

/// Counts authority changes on this device. Desktop setup manages one instance; without it no
/// live check is ever reused.
#[derive(Debug, Default)]
pub(crate) struct ChatAuthorityEpoch(AtomicU64);

impl ChatAuthorityEpoch {
    /// The current epoch.
    #[cfg(desktop)]
    pub(crate) fn current(&self) -> u64 {
        self.0.load(Ordering::Acquire)
    }

    /// Records one authority change.
    pub(crate) fn bump(&self) {
        self.0.fetch_add(1, Ordering::AcqRel);
    }
}

/// Records an authority change on `app`, if it manages an epoch.
pub(crate) fn bump_authority_epoch<R: Runtime>(app: &AppHandle<R>) {
    if let Some(epoch) = app.try_state::<ChatAuthorityEpoch>() {
        epoch.bump();
    }
}

/// The current epoch of `app`, or `None` when it manages none.
#[cfg(desktop)]
pub(crate) fn current_authority_epoch<R: Runtime>(app: &AppHandle<R>) -> Option<u64> {
    app.try_state::<ChatAuthorityEpoch>()
        .map(|epoch| epoch.current())
}

/// Records an authority change when a sync apply changed any Chat or Projects table, since
/// either can remove a membership, revoke an authorization, or archive a project.
pub(crate) fn on_sync_changes<R: Runtime>(app: &AppHandle<R>, tables: &[String]) {
    if touches_authority(tables) {
        bump_authority_epoch(app);
    }
}

fn touches_authority(tables: &[String]) -> bool {
    tables
        .iter()
        .any(|table| table.starts_with("chat_") || table.starts_with("project"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_authorization_epoch_counts_each_bump() {
        let epoch = ChatAuthorityEpoch::default();
        let first = epoch.0.load(Ordering::Acquire);
        epoch.bump();
        epoch.bump();
        assert_eq!(epoch.0.load(Ordering::Acquire), first + 2);
    }

    #[test]
    fn live_authorization_epoch_bumps_for_chat_and_project_sync_changes_only() {
        assert!(touches_authority(&[
            "chat_conversation_memberships".to_string()
        ]));
        assert!(touches_authority(&["projects".to_string()]));
        assert!(!touches_authority(&[
            "quick_notes".to_string(),
            "themes".to_string()
        ]));
        assert!(!touches_authority(&[]));
    }
}

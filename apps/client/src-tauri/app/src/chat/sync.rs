//! Chat work that follows rows sync applied.
//!
//! The post-apply hook runs `ganbaru_chat::sync_effects::after_materialize` inside the engine
//! transaction with only the connection and a change sink, so the headless Android pass runs it
//! too: it creates the device-local audience state of arrived conversations, bumps the audience
//! counters an arrived membership affects, and revokes the authorizations that the current app,
//! project, and channel access, the memberships, and the agents' archived state no longer
//! cover. It reports each agent it revoked and each conversation the apply deleted, whose
//! cascade removed the authorizations and runs that targeted it. On desktop the app host hands
//! those reports to [`on_sync_notices`], which wakes the access revocation drain so the revoked
//! runs and the runs whose authorization is gone stop. Push approvals replicate, so the hook
//! only reports the channels whose approvals lapsed, and desktop revokes them once the apply
//! ends. A work assignment that settled on another device ends its local dispatch job and
//! revokes its authorizations the same way, so the executing device stops its run. Queued
//! assignments and pending inputs are reported too, and desktop dispatch starts the ones whose
//! executing device is this one. Started assignments are reported so a device whose concurrent
//! start merged onto another executing device stops its own run. Android never holds an active
//! authorization, since runs execute only on desktop, so it has no drain to wake. Dispatch and
//! the readers of agents read the current rows when they run, and views learn about the changed
//! tables from the applied tables event.

use ganbaru_chat::sync_effects::{self, ChatSyncEffects};
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::chat;
use ganbaru_sync::{ChangeSink, HookError, HookFuture, Hooks, MaterializeHook, rebuild_every_key};
use sqlx::SqliteConnection;
use std::path::Path;

/// Checks a received `chat/attachments` file before the asset store keeps it: its bytes must
/// carry the signature of the content type its reference names.
pub(crate) fn check_synced_attachment(asset: &AssetRef, path: &Path) -> Result<(), String> {
    ganbaru_chat::attachment_files::check_received_file(asset.content_type(), path)
}

/// Change notice topic of an agent whose authorizations an apply revoked;
/// the key is the agent participant id.
pub(crate) const CHAT_ACCESS_REVOKED_TOPIC: &str = "chat-access-revoked";

/// Change notice topic of a conversation that still holds push approvals an apply left lapsed;
/// the key is the conversation id.
pub(crate) const CHAT_PUSH_APPROVALS_LAPSED_TOPIC: &str = "chat-push-approvals-lapsed";

/// Change notice topic of a conversation an apply deleted; the key is the conversation id.
pub(crate) const CHAT_CONVERSATION_REMOVED_TOPIC: &str = "chat-conversation-removed";

/// Change notice topic of a work assignment an apply deleted; the key is the assignment id.
pub(crate) const CHAT_ASSIGNMENT_REMOVED_TOPIC: &str = "chat-assignment-removed";

/// Change notice topic of a queued work assignment that arrived or changed; the key is the
/// assignment id.
pub(crate) const CHAT_ASSIGNMENT_QUEUED_TOPIC: &str = "chat-assignment-queued";

/// Change notice topic of a work assignment with an arrived input pending delivery; the key is
/// the assignment id.
pub(crate) const CHAT_ASSIGNMENT_INPUT_TOPIC: &str = "chat-assignment-input";

/// Change notice topic of a started, unsettled work assignment that arrived or changed; the key
/// is the assignment id.
pub(crate) const CHAT_ASSIGNMENT_STARTED_TOPIC: &str = "chat-assignment-started";

/// Change notice topic of a delegated work assignment that arrived in a state its parent hears
/// about; the key is the delegated assignment id.
pub(crate) const CHAT_DELEGATION_SETTLED_TOPIC: &str = "chat-delegation-settled";

/// Replicated tables whose applied rows feed device-local Chat state, each registered with
/// [`ChatApplyHook`].
const HOOKED_TABLES: [&TableSpec; 11] = [
    &chat::PARTICIPANTS,
    &chat::APP_SETTINGS,
    &chat::PROJECT_SETTINGS,
    &chat::CHANNELS,
    &chat::CONVERSATIONS,
    &chat::MEMBERSHIPS,
    &chat::CHANNEL_MEMBERSHIPS,
    &chat::MESSAGES,
    &chat::REVISIONS,
    &chat::ASSIGNMENTS,
    &chat::INPUTS,
];

/// Recomputes device-local Chat state after sync materializes Chat rows and reports revocations.
struct ChatApplyHook;

fn hook_error(error: ganbaru_chat_contracts::models::ChatError) -> HookError {
    HookError::from(error.to_string())
}

/// Reports the work `effects` leaves for services outside the engine transaction.
fn report_effects(effects: ChatSyncEffects, changes: &mut dyn ChangeSink) {
    for agent_id in effects.revoked_agents {
        changes.report(CHAT_ACCESS_REVOKED_TOPIC, agent_id);
    }
    for conversation_id in effects.lapsed_push_approvals {
        changes.report(CHAT_PUSH_APPROVALS_LAPSED_TOPIC, conversation_id);
    }
    for conversation_id in effects.removed_conversations {
        changes.report(CHAT_CONVERSATION_REMOVED_TOPIC, conversation_id);
    }
    for assignment_id in effects.removed_assignments {
        changes.report(CHAT_ASSIGNMENT_REMOVED_TOPIC, assignment_id);
    }
    for assignment_id in effects.queued_assignments {
        changes.report(CHAT_ASSIGNMENT_QUEUED_TOPIC, assignment_id);
    }
    for assignment_id in effects.started_assignments {
        changes.report(CHAT_ASSIGNMENT_STARTED_TOPIC, assignment_id);
    }
    for assignment_id in effects.routed_inputs {
        changes.report(CHAT_ASSIGNMENT_INPUT_TOPIC, assignment_id);
    }
    for assignment_id in effects.delegated_outcomes {
        changes.report(CHAT_DELEGATION_SETTLED_TOPIC, assignment_id);
    }
}

impl MaterializeHook for ChatApplyHook {
    fn after_materialize<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changed_keys: &'a [String],
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        Box::pin(async move {
            let effects = sync_effects::after_materialize(conn, table.name, changed_keys)
                .await
                .map_err(hook_error)?;
            report_effects(effects, changes);
            Ok(())
        })
    }

    fn rebuild_all<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        changes: &'a mut dyn ChangeSink,
    ) -> HookFuture<'a> {
        rebuild_every_key(self, conn, table, changes)
    }
}

/// `hooks` with the Chat post-apply hooks added.
pub(crate) fn with_hooks(hooks: Hooks) -> Hooks {
    HOOKED_TABLES.into_iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(ChatApplyHook))
    })
}

#[cfg(desktop)]
pub(crate) use waker::{AssignmentWaker, RevocationWaker, SyncedAssignmentWork, on_sync_notices};

#[cfg(desktop)]
mod waker {
    use super::{
        CHAT_ACCESS_REVOKED_TOPIC, CHAT_ASSIGNMENT_INPUT_TOPIC, CHAT_ASSIGNMENT_QUEUED_TOPIC,
        CHAT_ASSIGNMENT_REMOVED_TOPIC, CHAT_ASSIGNMENT_STARTED_TOPIC,
        CHAT_CONVERSATION_REMOVED_TOPIC, CHAT_DELEGATION_SETTLED_TOPIC,
        CHAT_PUSH_APPROVALS_LAPSED_TOPIC,
    };
    use ganbaru_sync::ChangeNotice;
    use std::collections::BTreeSet;
    use tauri::{AppHandle, Manager, Runtime};

    /// Assignment work that sync change notices hand to this device.
    #[derive(Clone, Debug, Default, Eq, PartialEq)]
    pub(crate) struct SyncedAssignmentWork {
        /// Queued assignments to start when this device executes them.
        pub(crate) queued: BTreeSet<String>,
        /// Started assignments, which this device stops when it runs them without being their
        /// executing device.
        pub(crate) started: BTreeSet<String>,
        /// Assignments with inputs pending delivery to the work this device runs, or left
        /// pending when its turn ended, for a follow-up to continue.
        pub(crate) routed: BTreeSet<String>,
        /// Delegated assignments whose outcome this device reports when it executes their
        /// parent.
        pub(crate) delegated: BTreeSet<String>,
        /// Whether an apply left unrevoked push approvals that lapsed, which this device
        /// revokes.
        pub(crate) push_approvals_lapsed: bool,
    }

    /// The assignment work `notices` report.
    pub(super) fn assignment_work(notices: &[ChangeNotice]) -> SyncedAssignmentWork {
        let mut work = SyncedAssignmentWork::default();
        for notice in notices {
            if notice.topic == CHAT_ASSIGNMENT_QUEUED_TOPIC {
                work.queued.insert(notice.key.clone());
            } else if notice.topic == CHAT_ASSIGNMENT_STARTED_TOPIC {
                work.started.insert(notice.key.clone());
            } else if notice.topic == CHAT_ASSIGNMENT_INPUT_TOPIC {
                work.routed.insert(notice.key.clone());
            } else if notice.topic == CHAT_DELEGATION_SETTLED_TOPIC {
                work.delegated.insert(notice.key.clone());
            } else if notice.topic == CHAT_PUSH_APPROVALS_LAPSED_TOPIC {
                work.push_approvals_lapsed = true;
            }
        }
        work
    }

    /// Starts synced assignment work on the running app.
    ///
    /// Dispatch needs the concrete desktop runtime while the sync host is generic over it, so
    /// desktop setup manages this waker with a handle that can start it.
    pub(crate) struct AssignmentWaker(Box<dyn Fn(SyncedAssignmentWork) + Send + Sync>);

    impl AssignmentWaker {
        /// A waker that starts the synced assignment work of `app`.
        pub(crate) fn new(app: tauri::AppHandle) -> Self {
            Self(Box::new(move |work| {
                crate::chat::organization::start_synced_assignment_work(&app, work);
            }))
        }
    }

    /// Whether sync change `notices` report revoked Chat authorizations, or deleted conversations
    /// or assignments, whose cascade removed the authorizations that targeted them.
    pub(super) fn access_revoked(notices: &[ChangeNotice]) -> bool {
        notices.iter().any(|notice| {
            notice.topic == CHAT_ACCESS_REVOKED_TOPIC
                || notice.topic == CHAT_CONVERSATION_REMOVED_TOPIC
                || notice.topic == CHAT_ASSIGNMENT_REMOVED_TOPIC
        })
    }

    /// Starts the access revocation drain of the running app.
    ///
    /// The drain needs the concrete desktop runtime while the sync host is generic over it, so
    /// desktop setup manages this waker with a handle that can start it.
    pub(crate) struct RevocationWaker(Box<dyn Fn() + Send + Sync>);

    impl RevocationWaker {
        /// A waker that starts the drain of `app`.
        pub(crate) fn new(app: tauri::AppHandle) -> Self {
            Self(Box::new(move || {
                crate::chat::revocation::start_job_drain(&app);
            }))
        }
    }

    /// Wakes the access revocation drain when sync change `notices` report revoked
    /// authorizations, so the runs they granted stop on this device, and hands queued
    /// assignments and pending inputs to dispatch, which acts only on the work this device
    /// executes.
    pub(crate) fn on_sync_notices<R: Runtime>(app: &AppHandle<R>, notices: &[ChangeNotice]) {
        if access_revoked(notices) {
            super::RevocationWake::wake_revocation_drain(app);
        }
        let work = assignment_work(notices);
        if work == SyncedAssignmentWork::default() {
            return;
        }
        if let Some(waker) = app.try_state::<AssignmentWaker>() {
            (waker.0)(work);
        }
    }

    /// Starts the drain through the waker desktop setup manages, if it is running.
    pub(super) fn wake<R: Runtime>(app: &AppHandle<R>) {
        if let Some(waker) = app.try_state::<RevocationWaker>() {
            (waker.0)();
        }
    }
}

/// Starts the access revocation drain after a change on this device that may have removed
/// authorizations by cascade, so the runs they granted stop now instead of at the next
/// unrelated drain. Every local command whose delete cascades into Chat conversations or
/// authorizations calls it after committing. Android never holds an active authorization, so
/// it has no drain and waking does nothing there.
pub(crate) trait RevocationWake {
    /// Starts the drain.
    fn wake_revocation_drain(&self);
}

impl<R: tauri::Runtime> RevocationWake for tauri::AppHandle<R> {
    fn wake_revocation_drain(&self) {
        crate::chat::authority_epoch::bump_authority_epoch(self);
        #[cfg(desktop)]
        waker::wake(self);
    }
}

#[cfg(test)]
mod tests;

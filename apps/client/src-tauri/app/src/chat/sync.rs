//! Chat work that follows rows sync applied.
//!
//! The post-apply hook runs `ganbaru_chat::sync_effects::after_materialize` inside the engine
//! transaction with only the connection and a change sink, so the headless Android pass runs it
//! too: it creates the device-local access state of arrived teammates and audience state of
//! arrived conversations, bumps the audience and access counters an arrived effective profile
//! revision, membership, or folder grant affects, and revokes authorizations a narrowing revision
//! or a removed or narrowed membership or grant no longer covers. A teammate or profile pointer
//! that copies restrictiveness differing from the revision it names has no effective revision,
//! so its teammates are refused; the hook revokes their authorizations and logs the pointer
//! rather than failing the apply, which would stop every later apply. It reports each teammate
//! it revoked and each conversation the apply deleted, whose cascade removed the authorizations
//! and runs that targeted it. On desktop the app host hands those reports to
//! [`on_sync_notices`], which wakes the access revocation drain so the revoked runs and the runs
//! whose authorization is gone stop. A work assignment that settled on another device ends its
//! local dispatch job and revokes its authorizations the same way, so the executing device stops
//! its run. Queued assignments and pending inputs are reported too, and desktop dispatch starts
//! the ones whose executing device is this one. Started assignments are reported so a device
//! whose concurrent start merged onto another executing device stops its own run. The hook also
//! settles concurrent folder grant edits: when one inherits the channel override and that
//! override is stricter than the explicit override the merge kept, the grant takes it. Android
//! never holds an active authorization, since runs execute only on desktop, so it has no drain to
//! wake. Dispatch and the readers of teammates and profiles read the effective rows
//! when they run, and views learn about the changed tables from the applied tables event.

use ganbaru_chat::sync_effects::{self, ChatSyncEffects};
use ganbaru_sync::assets::AssetRef;
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::chat;
use ganbaru_sync::manifest::vault::chat::folder_grant_group;
use ganbaru_sync::{
    ChangeSink, ConcurrentVersion, HookError, HookFuture, Hooks, MaterializeHook, SettleFuture,
};
use ganbaru_sync_contracts::{Field, GroupId, Value};
use sqlx::SqliteConnection;
use std::path::Path;

/// Checks a received `chat/attachments` file before the asset store keeps it: its bytes must
/// carry the signature of the content type its reference names.
pub(crate) fn check_synced_attachment(asset: &AssetRef, path: &Path) -> Result<(), String> {
    ganbaru_chat::attachment_files::check_received_file(asset.content_type(), path)
}

/// Change notice topic of a teammate whose authorizations an applied profile revision revoked;
/// the key is the teammate participant id.
pub(crate) const CHAT_ACCESS_REVOKED_TOPIC: &str = "chat-access-revoked";

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

/// Replicated tables whose applied rows feed device-local Chat state, each registered with
/// [`ChatApplyHook`].
const HOOKED_TABLES: [&TableSpec; 12] = [
    &chat::TEAMMATES,
    &chat::POLICY_REVISIONS,
    &chat::PROFILES,
    &chat::PROFILE_REVISIONS,
    &chat::CONVERSATIONS,
    &chat::MEMBERSHIPS,
    &chat::CHANNEL_MEMBERSHIPS,
    &chat::FOLDER_GRANTS,
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
///
/// A mismatched pointer is logged rather than reported, since its teammates are already refused
/// and only an honest edit from another device can repair it.
fn report_effects(effects: ChatSyncEffects, changes: &mut dyn ChangeSink) {
    for owner_id in &effects.mismatched_pointers {
        eprintln!(
            "Synced Chat row {owner_id} copies restrictiveness that differs from the revision it \
             names, so its teammates have no effective access"
        );
    }
    for teammate_id in effects.revoked_teammates {
        changes.report(CHAT_ACCESS_REVOKED_TOPIC, teammate_id);
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

    fn settled_groups(&self, table: &'static TableSpec) -> &'static [GroupId] {
        if table.id == chat::FOLDER_GRANTS.id {
            &SETTLED_GRANT_GROUPS
        } else {
            &[]
        }
    }

    fn settle<'a>(
        &'a self,
        conn: &'a mut SqliteConnection,
        table: &'static TableSpec,
        key: &'a str,
        group: GroupId,
        versions: &'a [ConcurrentVersion],
    ) -> SettleFuture<'a> {
        Box::pin(async move {
            if table.id != chat::FOLDER_GRANTS.id || group != folder_grant_group::GRANT {
                return Ok(None);
            }
            settle_folder_grant(conn, key, versions)
                .await
                .map_err(hook_error)
        })
    }
}

/// Folder grant groups whose concurrent versions [`ChatApplyHook`] settles.
const SETTLED_GRANT_GROUPS: [GroupId; 1] = [folder_grant_group::GRANT];

/// Position of the runtime approval override in a folder grant's grant group value.
const GRANT_APPROVAL_FIELD: usize = 3;

/// Position of the revocation time in a folder grant's grant group value.
const GRANT_REVOKED_FIELD: usize = 4;

/// The approval override a grant group version holds, `None` when it inherits.
fn grant_approval(value: &Value) -> Option<&str> {
    match value.fields().get(GRANT_APPROVAL_FIELD) {
        Some(Field::Text(policy)) => Some(policy),
        _ => None,
    }
}

/// Settles the concurrent grant group `versions` of the folder grant `key`: a revocation
/// stands, and otherwise the winner takes the stricter channel override an inheriting version
/// follows, see [`sync_effects::settled_grant_approval`].
async fn settle_folder_grant(
    conn: &mut SqliteConnection,
    key: &str,
    versions: &[ConcurrentVersion],
) -> ganbaru_chat_contracts::models::ChatResult<Option<Value>> {
    let Some(winner) = versions.iter().find(|version| version.won) else {
        return Ok(None);
    };
    if !matches!(
        winner.value.fields().get(GRANT_REVOKED_FIELD),
        None | Some(Field::Null)
    ) {
        return Ok(None);
    }
    let approvals: Vec<Option<&str>> = versions
        .iter()
        .map(|version| grant_approval(&version.value))
        .collect();
    let Some(policy) =
        sync_effects::settled_grant_approval(conn, key, grant_approval(&winner.value), &approvals)
            .await?
    else {
        return Ok(None);
    };
    let mut fields = winner.value.fields().to_vec();
    let Some(approval) = fields.get_mut(GRANT_APPROVAL_FIELD) else {
        return Ok(None);
    };
    *approval = Field::Text(policy);
    Ok(Value::new(fields).ok())
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
        CHAT_CONVERSATION_REMOVED_TOPIC,
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
        /// Assignments with inputs pending delivery to the work this device runs.
        pub(crate) routed: BTreeSet<String>,
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
        if work.queued.is_empty() && work.started.is_empty() && work.routed.is_empty() {
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
        #[cfg(desktop)]
        waker::wake(self);
    }
}

#[cfg(test)]
mod tests;

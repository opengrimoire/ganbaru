//! Chat work that follows rows sync applied.
//!
//! The post-apply hook runs `ganbaru_chat::sync_effects::after_materialize` inside the engine
//! transaction with only the connection and a change sink, so the headless Android pass runs it
//! too: it creates the device-local access state of arrived teammates, bumps the audience and
//! access counters an arrived effective profile revision affects, and revokes authorizations a
//! narrowing revision no longer allows. It refuses the apply when a teammate or profile pointer
//! copies restrictiveness that differs from the revision it names. It reports each teammate it
//! revoked. On desktop the app
//! host hands those reports to [`on_sync_notices`], which wakes the access revocation drain so the
//! revoked runs stop. Dispatch and the readers of teammates and profiles read the effective rows
//! when they run, and views learn about the changed tables from the applied tables event.

use ganbaru_chat::sync_effects::{self, ChatSyncEffects};
use ganbaru_sync::manifest::TableSpec;
use ganbaru_sync::manifest::vault::chat;
use ganbaru_sync::{ChangeSink, HookError, HookFuture, Hooks, MaterializeHook};
use sqlx::SqliteConnection;

/// Change notice topic of a teammate whose authorizations an applied profile revision revoked;
/// the key is the teammate participant id.
pub(crate) const CHAT_ACCESS_REVOKED_TOPIC: &str = "chat-access-revoked";

/// Replicated tables whose applied rows feed device-local Chat state, each registered with
/// [`ChatApplyHook`].
const HOOKED_TABLES: [&TableSpec; 4] = [
    &chat::TEAMMATES,
    &chat::POLICY_REVISIONS,
    &chat::PROFILES,
    &chat::PROFILE_REVISIONS,
];

/// Recomputes device-local Chat state after sync materializes Chat rows and reports revocations.
struct ChatApplyHook;

fn hook_error(error: ganbaru_chat_contracts::models::ChatError) -> HookError {
    HookError::from(error.to_string())
}

/// Reports the work `effects` leaves for services outside the engine transaction.
fn report_effects(effects: ChatSyncEffects, changes: &mut dyn ChangeSink) {
    for teammate_id in effects.revoked_teammates {
        changes.report(CHAT_ACCESS_REVOKED_TOPIC, teammate_id);
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
}

/// `hooks` with the Chat post-apply hooks added.
pub(crate) fn with_hooks(hooks: Hooks) -> Hooks {
    HOOKED_TABLES.into_iter().fold(hooks, |hooks, table| {
        hooks.with(table.id, Box::new(ChatApplyHook))
    })
}

#[cfg(desktop)]
pub(crate) use waker::{RevocationWaker, on_sync_notices};

#[cfg(desktop)]
mod waker {
    use super::CHAT_ACCESS_REVOKED_TOPIC;
    use ganbaru_sync::ChangeNotice;
    use tauri::{AppHandle, Manager, Runtime};

    /// Whether sync change `notices` report revoked Chat authorizations.
    pub(super) fn access_revoked(notices: &[ChangeNotice]) -> bool {
        notices
            .iter()
            .any(|notice| notice.topic == CHAT_ACCESS_REVOKED_TOPIC)
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
    /// authorizations, so the runs they granted stop on this device.
    pub(crate) fn on_sync_notices<R: Runtime>(app: &AppHandle<R>, notices: &[ChangeNotice]) {
        if !access_revoked(notices) {
            return;
        }
        if let Some(waker) = app.try_state::<RevocationWaker>() {
            (waker.0)();
        }
    }
}

#[cfg(test)]
mod tests;

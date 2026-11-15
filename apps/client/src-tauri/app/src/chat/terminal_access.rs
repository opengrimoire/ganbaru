//! In-memory draft thread reservations for terminals.
//!
//! An unsaved draft thread has no folder in SQLite yet, so Rust records the
//! folder it may open terminals in until the thread is persisted or discarded.

use ganbaru_chat_contracts::models::{
    ChatError, ChatErrorCode, ChatResult, ChatThreadId, ProjectWorkingFolderId,
};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// Most unsaved draft threads that may hold a terminal reservation at once.
pub const MAX_DRAFT_THREAD_RESERVATIONS: usize = 256;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct DraftReservationKey {
    vault_id: String,
    thread_id: ChatThreadId,
}

#[derive(Default)]
struct TerminalAccessState {
    reservations: BTreeMap<DraftReservationKey, ProjectWorkingFolderId>,
}

/// Managed state holding this launch's draft thread reservations.
#[derive(Default)]
pub struct ChatTerminalAccess {
    state: Mutex<TerminalAccessState>,
}

impl ChatTerminalAccess {
    /// Drops every reservation for a folder that was unbound, rebound, archived, or removed.
    pub fn clear_folder(&self, working_folder_id: &ProjectWorkingFolderId) -> ChatResult<()> {
        self.lock()?
            .reservations
            .retain(|_, folder| folder != working_folder_id);
        Ok(())
    }

    /// Binds an unsaved draft thread to the folder it may open terminals in.
    ///
    /// At the limit, reservations of drafts without a live terminal are
    /// released first; the frontend reserves again before its next terminal
    /// request, so only drafts that still hold terminals count.
    pub fn reserve(
        &self,
        vault_id: &str,
        thread_id: &ChatThreadId,
        working_folder_id: &ProjectWorkingFolderId,
        has_live_terminal: impl Fn(&ChatThreadId) -> bool,
    ) -> ChatResult<()> {
        let mut state = self.lock()?;
        let key = DraftReservationKey {
            vault_id: vault_id.to_string(),
            thread_id: thread_id.clone(),
        };
        match state.reservations.get(&key) {
            Some(folder) if folder == working_folder_id => return Ok(()),
            Some(_) => return Err(reservation_mismatch_error()),
            None => {}
        }
        if state.reservations.len() >= MAX_DRAFT_THREAD_RESERVATIONS {
            state
                .reservations
                .retain(|reserved, _| has_live_terminal(&reserved.thread_id));
        }
        if state.reservations.len() >= MAX_DRAFT_THREAD_RESERVATIONS {
            return Err(ChatError::new(
                ChatErrorCode::Busy,
                "Too many unsaved Chat threads have terminals",
                true,
            ));
        }
        state.reservations.insert(key, working_folder_id.clone());
        Ok(())
    }

    /// Returns the folder an unsaved draft thread was reserved for, if any.
    pub fn reserved_folder(
        &self,
        vault_id: &str,
        thread_id: &ChatThreadId,
    ) -> ChatResult<Option<ProjectWorkingFolderId>> {
        Ok(self
            .lock()?
            .reservations
            .get(&DraftReservationKey {
                vault_id: vault_id.to_string(),
                thread_id: thread_id.clone(),
            })
            .cloned())
    }

    /// Releases a draft reservation once the thread is persisted or discarded.
    pub fn release(&self, vault_id: &str, thread_id: &ChatThreadId) -> ChatResult<()> {
        self.lock()?.reservations.remove(&DraftReservationKey {
            vault_id: vault_id.to_string(),
            thread_id: thread_id.clone(),
        });
        Ok(())
    }

    fn lock(&self) -> ChatResult<std::sync::MutexGuard<'_, TerminalAccessState>> {
        self.state.lock().map_err(|_| {
            ChatError::new(
                ChatErrorCode::Internal,
                "Chat terminal access state is unavailable",
                false,
            )
        })
    }
}

/// Error for a draft thread already reserved for a different folder.
pub fn reservation_mismatch_error() -> ChatError {
    ChatError::new(
        ChatErrorCode::Permission,
        "Chat terminal workspace does not match the thread",
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(id: &str) -> ProjectWorkingFolderId {
        ProjectWorkingFolderId::new(id).expect("folder ID should be valid")
    }

    fn thread(id: &str) -> ChatThreadId {
        ChatThreadId::new(id).expect("thread ID should be valid")
    }

    fn live(_: &ChatThreadId) -> bool {
        true
    }

    #[test]
    fn clearing_a_folder_drops_its_reservations_only() {
        let access = ChatTerminalAccess::default();
        access
            .reserve("vault", &thread("draft-a"), &folder("folder-a"), live)
            .expect("reserve");
        access
            .reserve("vault", &thread("draft-b"), &folder("folder-b"), live)
            .expect("reserve");

        access.clear_folder(&folder("folder-a")).expect("clear");

        assert_eq!(
            access
                .reserved_folder("vault", &thread("draft-a"))
                .expect("read"),
            None
        );
        assert_eq!(
            access
                .reserved_folder("vault", &thread("draft-b"))
                .expect("read"),
            Some(folder("folder-b"))
        );
    }

    #[test]
    fn reservations_bind_one_folder_and_are_bounded() {
        let access = ChatTerminalAccess::default();
        access
            .reserve("vault", &thread("draft"), &folder("folder-a"), live)
            .expect("reserve");
        access
            .reserve("vault", &thread("draft"), &folder("folder-a"), live)
            .expect("repeat reservation is idempotent");
        let mismatch = access
            .reserve("vault", &thread("draft"), &folder("folder-b"), live)
            .expect_err("a draft cannot move to another folder");
        assert_eq!(mismatch.code, ChatErrorCode::Permission);
        assert_eq!(
            access
                .reserved_folder("other-vault", &thread("draft"))
                .expect("read"),
            None
        );

        for index in 1..MAX_DRAFT_THREAD_RESERVATIONS {
            access
                .reserve(
                    "vault",
                    &thread(&format!("draft-{index}")),
                    &folder("f"),
                    live,
                )
                .expect("reserve under the limit");
        }
        let full = access
            .reserve("vault", &thread("one-more"), &folder("f"), live)
            .expect_err("the limit is enforced");
        assert_eq!(full.code, ChatErrorCode::Busy);
        access.release("vault", &thread("draft")).expect("release");
        access
            .reserve("vault", &thread("one-more"), &folder("f"), live)
            .expect("released capacity is reusable");
    }

    #[test]
    fn full_reservations_release_drafts_without_live_terminals() {
        let access = ChatTerminalAccess::default();
        for index in 0..MAX_DRAFT_THREAD_RESERVATIONS {
            access
                .reserve(
                    "vault",
                    &thread(&format!("draft-{index}")),
                    &folder("f"),
                    live,
                )
                .expect("reserve under the limit");
        }
        access
            .reserve("vault", &thread("next"), &folder("f"), |thread_id| {
                thread_id.as_str() == "draft-0"
            })
            .expect("idle drafts make room");
        assert_eq!(
            access
                .reserved_folder("vault", &thread("draft-0"))
                .expect("read"),
            Some(folder("f"))
        );
        assert_eq!(
            access
                .reserved_folder("vault", &thread("draft-1"))
                .expect("read"),
            None
        );
    }
}

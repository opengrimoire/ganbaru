//! Coalesced vault lifecycle requests and immediate callback revocation.

use tokio::sync::watch;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LifecycleIntent {
    Active,
    Freeze,
    Resume,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LifecycleRequest {
    pub revision: u64,
    pub intent: LifecycleIntent,
}

#[derive(Clone)]
pub(crate) struct LifecycleControl(watch::Sender<LifecycleRequest>);

impl LifecycleControl {
    pub fn new() -> Self {
        Self(
            watch::channel(LifecycleRequest {
                revision: 0,
                intent: LifecycleIntent::Active,
            })
            .0,
        )
    }

    pub fn subscribe(&self) -> watch::Receiver<LifecycleRequest> {
        self.0.subscribe()
    }

    /// Capture a lifecycle identity without retaining a publication lock.
    #[cfg(any(not(target_os = "ios"), test))]
    pub fn current(&self) -> LifecycleRequest {
        *self.0.borrow()
    }

    pub fn request(&self, intent: LifecycleIntent) -> Result<u64, String> {
        let mut revision = None;
        self.0.send_if_modified(|current| {
            let Some(next) = current.revision.checked_add(1) else {
                return false;
            };
            *current = LifecycleRequest {
                revision: next,
                intent,
            };
            revision = Some(next);
            true
        });
        revision.ok_or_else(|| "Native vault lifecycle revision is exhausted".to_string())
    }

    pub fn is_revoked(&self) -> bool {
        self.0.borrow().intent != LifecycleIntent::Active
    }

    pub fn matches(&self, revision: u64, intent: LifecycleIntent) -> bool {
        let current = *self.0.borrow();
        current.revision == revision && current.intent == intent
    }

    /// Only the requested resume may reopen admission after its worker drains.
    pub fn complete_resume(&self, revision: u64) -> bool {
        self.0.send_if_modified(|current| {
            if current.revision != revision || current.intent != LifecycleIntent::Resume {
                return false;
            }
            current.intent = LifecycleIntent::Active;
            true
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_lifecycle_resume_supersedes_a_freeze_still_waiting_in_the_command_queue() {
        let lifecycle = LifecycleControl::new();
        let freeze = lifecycle.request(LifecycleIntent::Freeze).unwrap();
        assert!(lifecycle.is_revoked());
        assert!(lifecycle.matches(freeze, LifecycleIntent::Freeze));
        let resume = lifecycle.request(LifecycleIntent::Resume).unwrap();
        assert!(!lifecycle.matches(freeze, LifecycleIntent::Freeze));
        assert!(lifecycle.is_revoked());
        assert!(!lifecycle.complete_resume(freeze));
        assert!(lifecycle.complete_resume(resume));
        assert!(!lifecycle.is_revoked());
    }

    #[test]
    fn native_lifecycle_old_resume_cannot_reopen_a_newer_freeze_or_resume() {
        let lifecycle = LifecycleControl::new();
        let old_resume = lifecycle.request(LifecycleIntent::Resume).unwrap();
        let freeze = lifecycle.request(LifecycleIntent::Freeze).unwrap();
        assert!(!lifecycle.complete_resume(old_resume));
        assert!(lifecycle.matches(freeze, LifecycleIntent::Freeze));
        let resume = lifecycle.request(LifecycleIntent::Resume).unwrap();
        assert!(!lifecycle.complete_resume(old_resume));
        assert!(lifecycle.is_revoked());
        assert!(lifecycle.complete_resume(resume));
        assert!(!lifecycle.complete_resume(resume));
    }

    #[test]
    fn native_lifecycle_retains_only_the_latest_request_and_rejects_revision_overflow() {
        let lifecycle = LifecycleControl::new();
        let receiver = lifecycle.subscribe();
        lifecycle.request(LifecycleIntent::Freeze).unwrap();
        let latest = lifecycle.request(LifecycleIntent::Resume).unwrap();
        assert_eq!(lifecycle.current().revision, latest);
        assert_eq!(receiver.borrow().revision, latest);
        assert_eq!(receiver.borrow().intent, LifecycleIntent::Resume);
        lifecycle.0.send_modify(|state| state.revision = u64::MAX);
        assert!(lifecycle.request(LifecycleIntent::Freeze).is_err());
        assert!(lifecycle.is_revoked());
        assert!(!lifecycle.complete_resume(latest));
    }
}

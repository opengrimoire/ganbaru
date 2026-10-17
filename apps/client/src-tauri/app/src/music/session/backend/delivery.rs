//! One retained Android delivery until execution, rejection, or confirmed cancellation.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::sync::oneshot;

type Authority = dyn Fn() -> bool + Send + Sync;
type DeliveryResult = Result<(), String>;

#[derive(Clone, Copy)]
pub(super) enum DeliveryKind {
    Load,
    Control,
    Stop,
}

#[derive(Clone)]
struct Scope {
    id: i64,
    canceled: Arc<AtomicBool>,
    authority: Arc<Authority>,
}

impl Scope {
    fn current(&self) -> bool {
        !self.canceled.load(Ordering::Acquire)
            && (self.authority)()
            && !self.canceled.load(Ordering::Acquire)
    }
}

struct Pending {
    scope: Scope,
    reply: oneshot::Sender<DeliveryResult>,
}

#[derive(Default)]
struct Inner {
    next_id: i64,
    pending: Option<Pending>,
    load: Option<Scope>,
}

#[derive(Default)]
pub(super) struct Registry {
    inner: Mutex<Inner>,
}

impl Registry {
    pub fn begin(
        self: &Arc<Self>,
        kind: DeliveryKind,
        authority: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Result<Delivery, String> {
        let mut inner = self
            .inner
            .lock()
            .map_err(|_| "Android Music delivery lock is unavailable")?;
        if inner.pending.is_some() {
            return Err("Earlier Android Music delivery is still draining".into());
        }
        inner.next_id = inner
            .next_id
            .checked_add(1)
            .ok_or("Android Music delivery identity is exhausted")?;
        let scope = Scope {
            id: inner.next_id,
            canceled: Arc::new(AtomicBool::new(false)),
            authority: Arc::new(authority),
        };
        match kind {
            DeliveryKind::Load | DeliveryKind::Stop => {
                if let Some(prior) = inner.load.take() {
                    prior.canceled.store(true, Ordering::Release);
                }
                if matches!(kind, DeliveryKind::Load) {
                    inner.load = Some(scope.clone());
                }
            }
            DeliveryKind::Control => (),
        }
        let (reply, response) = oneshot::channel();
        inner.pending = Some(Pending {
            scope: scope.clone(),
            reply,
        });
        Ok(Delivery {
            registry: Arc::clone(self),
            scope,
            response: Some(response),
            armed: true,
        })
    }

    /// Platform callbacks never wait behind another owner or access storage.
    pub fn current(&self, id: i64) -> bool {
        if id <= 0 {
            return false;
        }
        let scope = {
            let Ok(inner) = self.inner.try_lock() else {
                return false;
            };
            inner
                .pending
                .as_ref()
                .map(|pending| &pending.scope)
                .filter(|scope| scope.id == id)
                .or_else(|| inner.load.as_ref().filter(|scope| scope.id == id))
                .cloned()
        };
        scope.is_some_and(|scope| scope.current())
    }

    pub fn canceled_pending(&self) -> Result<Option<i64>, String> {
        let inner = self
            .inner
            .lock()
            .map_err(|_| "Android Music delivery lock is unavailable")?;
        Ok(inner
            .pending
            .as_ref()
            .filter(|pending| pending.scope.canceled.load(Ordering::Acquire))
            .map(|pending| pending.scope.id))
    }

    /// Completion must come from executed SDK work or confirmed removal of canceled work.
    pub fn complete(&self, id: i64, result: DeliveryResult) -> Result<bool, String> {
        let pending = {
            let mut inner = self
                .inner
                .lock()
                .map_err(|_| "Android Music delivery lock is unavailable")?;
            if inner
                .pending
                .as_ref()
                .is_none_or(|pending| pending.scope.id != id)
            {
                return Ok(false);
            }
            let pending = inner.pending.take().expect("matching pending delivery");
            if result.is_err() {
                pending.scope.canceled.store(true, Ordering::Release);
                if inner.load.as_ref().is_some_and(|scope| scope.id == id) {
                    inner.load = None;
                }
            }
            pending
        };
        let _ = pending.reply.send(result);
        Ok(true)
    }
}

pub(super) struct Delivery {
    registry: Arc<Registry>,
    scope: Scope,
    response: Option<oneshot::Receiver<DeliveryResult>>,
    armed: bool,
}

impl Delivery {
    pub fn id(&self) -> i64 {
        self.scope.id
    }

    pub fn rejected(&self, error: String) -> Result<(), String> {
        self.registry.complete(self.id(), Err(error)).map(|_| ())
    }

    pub async fn wait(mut self, timeout: Duration) -> DeliveryResult {
        let response = self.response.take().expect("one delivery waiter");
        match tokio::time::timeout(timeout, response).await {
            Ok(Ok(result)) => {
                self.armed = false;
                result
            }
            Ok(Err(error)) => Err(format!("Android Music execution result was lost: {error}")),
            Err(_) => Err("Android Music did not acknowledge execution in time".into()),
        }
    }
}

impl Drop for Delivery {
    fn drop(&mut self) {
        if self.armed {
            self.scope.canceled.store(true, Ordering::Release);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn rejected_admission_releases_its_slot_and_revokes_source_resolution() {
        let registry = Arc::new(Registry::default());
        let delivery = registry.begin(DeliveryKind::Load, || true).unwrap();
        let id = delivery.id();
        delivery
            .rejected("queue rejected before admission".into())
            .unwrap();
        assert!(!registry.current(id));
        assert_eq!(
            delivery.wait(Duration::from_secs(1)).await.unwrap_err(),
            "queue rejected before admission"
        );
        assert!(registry.begin(DeliveryKind::Control, || true).is_ok());
    }

    #[test]
    fn cancellation_during_the_cached_predicate_cannot_return_current() {
        let canceled = Arc::new(AtomicBool::new(false));
        let revoke = Arc::clone(&canceled);
        let scope = Scope {
            id: 1,
            canceled,
            authority: Arc::new(move || {
                revoke.store(true, Ordering::Release);
                true
            }),
        };
        assert!(!scope.current());
    }

    #[tokio::test]
    async fn replacing_load_revokes_even_a_source_scope_cloned_before_replacement() {
        let registry = Arc::new(Registry::default());
        let old = registry.begin(DeliveryKind::Load, || true).unwrap();
        registry.complete(old.id(), Ok(())).unwrap();
        old.wait(Duration::from_secs(1)).await.unwrap();
        let old_scope = registry
            .inner
            .lock()
            .unwrap()
            .load
            .as_ref()
            .unwrap()
            .clone();
        assert!(old_scope.current());
        let _replacement = registry.begin(DeliveryKind::Load, || true).unwrap();
        assert!(!old_scope.current());
    }

    #[test]
    fn callback_contention_fails_closed_and_cancellation_cannot_reuse_a_delivery_identity() {
        let registry = Arc::new(Registry::default());
        let old = registry.begin(DeliveryKind::Load, || true).unwrap();
        let id = old.id();
        {
            let _writing = registry.inner.lock().unwrap();
            assert!(!registry.current(id));
        }
        drop(old);
        registry.complete(id, Err("drained".into())).unwrap();
        let replacement = registry.begin(DeliveryKind::Load, || true).unwrap();
        assert!(replacement.id() > id);
        assert!(!registry.current(id));
        assert!(registry.current(replacement.id()));
        assert!(!registry.complete(id, Ok(())).unwrap());
    }

    #[tokio::test]
    async fn an_admitted_delivery_keeps_its_slot_until_actual_execution() {
        let registry = Arc::new(Registry::default());
        let delivery = registry.begin(DeliveryKind::Control, || true).unwrap();
        let id = delivery.id();
        assert!(registry.current(id));
        assert!(registry.begin(DeliveryKind::Control, || true).is_err());
        assert!(!registry.complete(id + 1, Ok(())).unwrap());
        registry.complete(id, Ok(())).unwrap();
        delivery.wait(Duration::from_secs(1)).await.unwrap();
        assert!(!registry.current(id));
        assert!(registry.begin(DeliveryKind::Control, || true).is_ok());
    }

    #[tokio::test]
    async fn timeout_revokes_late_work_but_keeps_the_slot_until_confirmed_drain() {
        let registry = Arc::new(Registry::default());
        let delivery = registry.begin(DeliveryKind::Load, || true).unwrap();
        let id = delivery.id();
        assert!(delivery.wait(Duration::ZERO).await.is_err());
        assert!(!registry.current(id));
        assert_eq!(registry.canceled_pending().unwrap(), Some(id));
        assert!(registry.begin(DeliveryKind::Stop, || true).is_err());
        registry
            .complete(id, Err("confirmed canceled".into()))
            .unwrap();
        assert!(registry.begin(DeliveryKind::Stop, || true).is_ok());
    }

    #[test]
    fn cancellation_before_dispatch_and_live_revocation_cannot_reopen_on_a_later_resume() {
        let registry = Arc::new(Registry::default());
        let current = Arc::new(AtomicBool::new(true));
        let snapshot = Arc::clone(&current);
        let delivery = registry
            .begin(DeliveryKind::Load, move || snapshot.load(Ordering::Acquire))
            .unwrap();
        let id = delivery.id();
        assert!(registry.current(id));
        current.store(false, Ordering::Release);
        assert!(!registry.current(id));
        drop(delivery);
        current.store(true, Ordering::Release);
        assert!(!registry.current(id));
    }

    #[tokio::test]
    async fn loading_retains_only_its_latest_source_scope_after_acknowledgement() {
        let registry = Arc::new(Registry::default());
        let first = registry.begin(DeliveryKind::Load, || true).unwrap();
        let old = first.id();
        registry.complete(old, Ok(())).unwrap();
        first.wait(Duration::from_secs(1)).await.unwrap();
        assert!(registry.current(old));
        let pause = registry.begin(DeliveryKind::Control, || true).unwrap();
        registry.complete(pause.id(), Ok(())).unwrap();
        pause.wait(Duration::from_secs(1)).await.unwrap();
        assert!(registry.current(old));
        let newer = registry.begin(DeliveryKind::Load, || true).unwrap();
        assert!(!registry.current(old));
        newer.rejected("never admitted".into()).unwrap();
        assert!(newer.wait(Duration::from_secs(1)).await.is_err());
        assert!(!registry.current(old));
    }

    #[tokio::test]
    async fn stop_and_failed_execution_revoke_resolver_callbacks_without_accepting_stale_acknowledgements()
     {
        let registry = Arc::new(Registry::default());
        let load = registry.begin(DeliveryKind::Load, || true).unwrap();
        let old = load.id();
        registry.complete(old, Ok(())).unwrap();
        load.wait(Duration::from_secs(1)).await.unwrap();
        let stop = registry.begin(DeliveryKind::Stop, || true).unwrap();
        assert!(!registry.current(old));
        assert!(!registry.complete(old, Ok(())).unwrap());
        registry
            .complete(stop.id(), Err("decoder failed".into()))
            .unwrap();
        assert_eq!(
            stop.wait(Duration::from_secs(1)).await.unwrap_err(),
            "decoder failed"
        );
    }
}

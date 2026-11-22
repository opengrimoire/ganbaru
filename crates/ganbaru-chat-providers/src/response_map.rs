//! Response slots of the requests a line-delimited provider transport is waiting on.
//!
//! The reader delivers each response to the slot its request registered. When the transport
//! ends, the map closes: every waiting request fails with the closing failure, and a request
//! that registers afterwards is refused with the same failure instead of waiting for its
//! deadline on a connection that can no longer answer.

use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::oneshot;

/// Requests one connection may wait on at once.
const MAX_PENDING_RESPONSES: usize = 128;

/// The slot a response or the closing failure is sent to.
type ResponseSender<F> = oneshot::Sender<Result<Value, F>>;
/// The receiving end a request awaits.
type ResponseReceiver<F> = oneshot::Receiver<Result<Value, F>>;

enum ResponseMapState<F> {
    Open(HashMap<String, ResponseSender<F>>),
    /// The transport ended with this failure; no slot is registered again.
    Closed(F),
}

/// Why a request could not register its response slot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ResponseSlotRefusal<F> {
    /// The connection already waits on the most requests it allows.
    Full,
    /// The transport ended with this failure.
    Closed(F),
}

/// Pending response slots of one connection, shared by its client, reader, and writer.
pub(crate) struct ResponseMap<F> {
    state: Arc<Mutex<ResponseMapState<F>>>,
}

impl<F> Clone for ResponseMap<F> {
    fn clone(&self) -> Self {
        Self {
            state: Arc::clone(&self.state),
        }
    }
}

impl<F: Clone> ResponseMap<F> {
    pub(crate) fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(ResponseMapState::Open(HashMap::new()))),
        }
    }

    /// Registers the slot of request `key`, unless the map is full or closed.
    pub(crate) fn register(
        &self,
        key: String,
    ) -> Result<ResponseReceiver<F>, ResponseSlotRefusal<F>> {
        let mut state = self.lock();
        match &mut *state {
            ResponseMapState::Open(slots) if slots.len() >= MAX_PENDING_RESPONSES => {
                Err(ResponseSlotRefusal::Full)
            }
            ResponseMapState::Open(slots) => {
                let (sender, receiver) = oneshot::channel();
                slots.insert(key, sender);
                Ok(receiver)
            }
            ResponseMapState::Closed(failure) => Err(ResponseSlotRefusal::Closed(failure.clone())),
        }
    }

    /// Sends `result` to the slot of request `key`, if it is still waiting.
    pub(crate) fn deliver(&self, key: &str, result: Result<Value, F>) {
        if let Some(sender) = self.take(key) {
            let _ = sender.send(result);
        }
    }

    /// Takes the slot of request `key` out of the map.
    pub(crate) fn take(&self, key: &str) -> Option<ResponseSender<F>> {
        match &mut *self.lock() {
            ResponseMapState::Open(slots) => slots.remove(key),
            ResponseMapState::Closed(_) => None,
        }
    }

    /// Closes the map with `failure` and fails every waiting request with it. The first close
    /// wins, so the failure that ended the transport is the one later requests report.
    pub(crate) fn close(&self, failure: F) {
        let waiting = {
            let mut state = self.lock();
            if matches!(&*state, ResponseMapState::Closed(_)) {
                return;
            }
            match std::mem::replace(&mut *state, ResponseMapState::Closed(failure.clone())) {
                ResponseMapState::Open(slots) => slots.into_values().collect::<Vec<_>>(),
                ResponseMapState::Closed(_) => Vec::new(),
            }
        };
        for sender in waiting {
            let _ = sender.send(Err(failure.clone()));
        }
    }

    /// The map holds only slots and the closing failure, which stay consistent even when a
    /// holder panicked, so a poisoned lock is still used.
    fn lock(&self) -> MutexGuard<'_, ResponseMapState<F>> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[derive(Clone, Debug, Eq, PartialEq)]
    enum Failure {
        Closed,
        Oversized,
    }

    #[test]
    fn a_registered_slot_receives_its_response_once() {
        let map = ResponseMap::<Failure>::new();
        let mut receiver = map.register("1".to_string()).unwrap();
        map.deliver("1", Ok(json!({ "ok": true })));
        map.deliver("1", Ok(json!({ "ok": false })));

        assert_eq!(receiver.try_recv().unwrap(), Ok(json!({ "ok": true })));
        assert!(map.take("1").is_none());
    }

    #[test]
    fn closing_fails_waiting_requests_and_refuses_later_ones() {
        let map = ResponseMap::<Failure>::new();
        let mut waiting = map.register("1".to_string()).unwrap();
        map.close(Failure::Oversized);
        map.close(Failure::Closed);

        assert_eq!(waiting.try_recv().unwrap(), Err(Failure::Oversized));
        assert_eq!(
            map.register("2".to_string()).unwrap_err(),
            ResponseSlotRefusal::Closed(Failure::Oversized)
        );
        map.deliver("2", Ok(json!({})));
        assert!(map.take("2").is_none());
    }

    #[test]
    fn a_full_map_refuses_another_slot_until_one_settles() {
        let map = ResponseMap::<Failure>::new();
        let receivers = (0..MAX_PENDING_RESPONSES)
            .map(|index| map.register(index.to_string()).unwrap())
            .collect::<Vec<_>>();

        assert_eq!(
            map.register("extra".to_string()).unwrap_err(),
            ResponseSlotRefusal::Full
        );
        assert!(map.take("0").is_some());
        assert!(map.register("extra".to_string()).is_ok());
        drop(receivers);
    }
}

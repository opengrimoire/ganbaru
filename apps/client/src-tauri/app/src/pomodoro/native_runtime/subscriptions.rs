//! Bounded WebView presentation streams. Channels carry compact invalidations,
//! while snapshots remain in the serialized native owner.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::ipc::Channel;

use super::{FocusErrorCode, FocusExecutionError, FocusProjection, error};

const MAX_SUBSCRIPTIONS: usize = 8;
const SUBSCRIPTION_LEASE: Duration = Duration::from_secs(30);

/// Small enough for direct channel delivery without cached snapshot bodies.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FocusNotice {
    vault_generation: u64,
    revision: Option<i64>,
    available: bool,
    has_error: bool,
}

impl From<&FocusProjection> for FocusNotice {
    fn from(projection: &FocusProjection) -> Self {
        Self {
            vault_generation: projection.vault_generation,
            revision: projection
                .snapshot
                .as_ref()
                .map(|snapshot| snapshot.revision),
            available: projection.vault_id.is_some() && projection.snapshot.is_some(),
            has_error: projection.error.is_some(),
        }
    }
}

struct Subscription<T> {
    window_label: String,
    expires_at: Instant,
    transport: T,
    last: Option<FocusNotice>,
}

pub(super) struct Subscriptions<T> {
    entries: BTreeMap<String, Subscription<T>>,
}

impl<T> Default for Subscriptions<T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
        }
    }
}

impl<T> Subscriptions<T> {
    pub fn prune(&mut self, now: Instant) {
        self.entries
            .retain(|_, subscription| subscription.expires_at > now);
    }

    pub fn subscribe(
        &mut self,
        id: String,
        window_label: String,
        transport: T,
        now: Instant,
    ) -> Result<(), FocusExecutionError> {
        if id.is_empty() || id.len() > 128 || window_label.is_empty() || window_label.len() > 128 {
            return Err(error(
                FocusErrorCode::InvalidIntent,
                "Invalid Focus subscription identity",
            ));
        }
        self.prune(now);
        if let Some(existing) = self.entries.get(&id) {
            if existing.window_label != window_label {
                return Err(error(
                    FocusErrorCode::InvalidIntent,
                    "Focus subscription belongs to another window",
                ));
            }
        } else if self.entries.len() >= MAX_SUBSCRIPTIONS {
            return Err(error(
                FocusErrorCode::Busy,
                "Focus presentation subscription limit reached",
            ));
        }
        self.entries.insert(
            id,
            Subscription {
                window_label,
                expires_at: now + SUBSCRIPTION_LEASE,
                transport,
                last: None,
            },
        );
        Ok(())
    }

    pub fn renew(
        &mut self,
        id: &str,
        window_label: &str,
        now: Instant,
    ) -> Result<(), FocusExecutionError> {
        self.prune(now);
        let subscription = self
            .entries
            .get_mut(id)
            .filter(|subscription| subscription.window_label == window_label)
            .ok_or_else(|| {
                error(
                    FocusErrorCode::Unavailable,
                    "Focus subscription is expired or belongs to another window",
                )
            })?;
        subscription.expires_at = now + SUBSCRIPTION_LEASE;
        Ok(())
    }

    pub fn unsubscribe(&mut self, id: &str, window_label: &str) {
        if self
            .entries
            .get(id)
            .is_some_and(|subscription| subscription.window_label == window_label)
        {
            self.entries.remove(id);
        }
    }

    pub fn next_expiry(&self) -> Option<Instant> {
        self.entries
            .values()
            .map(|subscription| subscription.expires_at)
            .min()
    }
}

impl Subscriptions<Channel<FocusNotice>> {
    pub fn publish(&mut self, projection: &FocusProjection, now: Instant) {
        self.prune(now);
        let notice = FocusNotice::from(projection);
        self.entries.retain(|_, subscription| {
            if subscription.last.as_ref() == Some(&notice) {
                return true;
            }
            match subscription.transport.send(notice.clone()) {
                Ok(()) => {
                    subscription.last = Some(notice.clone());
                    true
                }
                Err(error) => {
                    eprintln!("Publish native Focus notice: {error}");
                    false
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focus_streams_bind_to_native_windows_and_expire_without_a_webview_acknowledgement() {
        let now = Instant::now();
        let mut streams = Subscriptions::<()>::default();
        for index in 0..MAX_SUBSCRIPTIONS {
            streams
                .subscribe(format!("stream-{index}"), "main".into(), (), now)
                .unwrap();
        }
        assert_eq!(
            streams
                .subscribe("overflow".into(), "main".into(), (), now)
                .unwrap_err()
                .code,
            FocusErrorCode::Busy
        );
        assert!(streams.renew("stream-0", "other-window", now).is_err());
        assert!(
            streams
                .subscribe("stream-0".into(), "other-window".into(), (), now)
                .is_err()
        );
        streams.unsubscribe("stream-0", "other-window");
        assert_eq!(streams.entries.len(), MAX_SUBSCRIPTIONS);
        streams
            .renew("stream-0", "main", now + Duration::from_secs(10))
            .unwrap();
        streams.prune(now + SUBSCRIPTION_LEASE);
        assert_eq!(streams.entries.len(), 1);
        assert!(streams.entries.contains_key("stream-0"));
        streams.prune(now + Duration::from_secs(40));
        assert!(streams.entries.is_empty());
        assert!(
            streams
                .renew("stream-0", "main", now + Duration::from_secs(40))
                .is_err()
        );
        streams
            .subscribe(
                "replacement".into(),
                "main".into(),
                (),
                now + Duration::from_secs(40),
            )
            .unwrap();
    }

    #[test]
    fn focus_notices_revoke_old_generations_without_carrying_execution_or_error_bodies() {
        let projection = FocusProjection {
            vault_id: Some("vault".into()),
            vault_generation: 3,
            snapshot: None,
            error: Some("diagnostic".repeat(1000)),
        };
        let notice = FocusNotice::from(&projection);
        assert!(!notice.available);
        assert!(notice.has_error);
        assert_eq!(notice.vault_generation, 3);
        let encoded = serde_json::to_vec(&notice).unwrap();
        assert!(encoded.len() < 256);
        assert!(!String::from_utf8(encoded).unwrap().contains("diagnostic"));
    }
}

//! Bounded, acknowledged WebView projections above the native session owner.

use super::models::{SessionEffect, SessionProjection, SessionQueueEntry};
use ganbaru_music_library::{MusicLibraryError, MusicLibraryResult};
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};
use tauri::ipc::Channel;

const MAX_SUBSCRIBERS: usize = 8;
pub(super) const MAX_PENDING_EFFECTS: usize = 32;
const SUBSCRIPTION_LEASE_MS: i64 = 30_000;
pub(super) const BROWSER_LEASE_MS: i64 = 5_000;

/// One ordered frame. The next frame waits for acknowledgement of this sequence.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SessionFrame {
    pub subscription_id: String,
    pub sequence: u64,
    pub effects_expire_at_ms: i64,
    pub snapshot: SessionProjection,
    pub effects: Vec<SessionEffect>,
}

/// Small channel notices never enqueue a full queue in Tauri's large-payload cache.
#[derive(Clone, Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub(crate) enum SessionNotice {
    Update {
        subscription_id: String,
        sequence: u64,
    },
    Invalidate {
        subscription_id: String,
    },
}

struct Subscriber {
    id: String,
    channel: Channel<SessionNotice>,
    sequence: u64,
    in_flight: Option<RetainedFrame>,
    dirty: bool,
    queue_key: Option<(String, u64)>,
    expires_at: i64,
    effects: Vec<SessionEffect>,
}

struct RetainedFrame {
    frame: SessionFrame,
    queue: Option<Arc<Vec<SessionQueueEntry>>>,
}

/// The configured primary window alone hosts browser decoders. Other windows read projections.
pub(super) struct Subscriptions {
    primary_window: String,
    subscribers: BTreeMap<String, Subscriber>,
    snapshot: Option<SessionProjection>,
    queue: Arc<Vec<SessionQueueEntry>>,
    browser_until: i64,
}

impl Subscriptions {
    pub(super) fn new(primary_window: String) -> Self {
        Self {
            primary_window,
            subscribers: BTreeMap::new(),
            snapshot: None,
            queue: Arc::new(Vec::new()),
            browser_until: 0,
        }
    }

    /// Revokes retained data and interrupts browser work without waiting for an old frame ACK.
    pub(super) fn invalidate_context(&mut self) {
        for (window, subscriber) in &self.subscribers {
            if let Err(error) = subscriber.channel.send(SessionNotice::Invalidate {
                subscription_id: subscriber.id.clone(),
            }) {
                eprintln!("invalidate Music subscription {window}: {error}");
            }
        }
        self.subscribers.clear();
        self.snapshot = None;
        self.queue = Arc::new(Vec::new());
        self.browser_until = 0;
    }

    pub(super) fn subscribe(
        &mut self,
        window: String,
        id: String,
        channel: Channel<SessionNotice>,
        now: i64,
    ) -> MusicLibraryResult<()> {
        self.expire(now);
        if !self.subscribers.contains_key(&window) && self.subscribers.len() >= MAX_SUBSCRIBERS {
            return Err(MusicLibraryError::conflict(
                "Music subscription capacity is exhausted",
            ));
        }
        if window == self.primary_window {
            self.browser_until = 0;
        }
        self.subscribers.insert(
            window,
            Subscriber {
                id,
                channel,
                sequence: 0,
                in_flight: None,
                dirty: true,
                queue_key: None,
                expires_at: now + SUBSCRIPTION_LEASE_MS,
                effects: Vec::new(),
            },
        );
        Ok(())
    }

    fn subscriber(&mut self, window: &str, id: &str) -> MusicLibraryResult<&mut Subscriber> {
        self.subscribers
            .get_mut(window)
            .filter(|subscriber| subscriber.id == id)
            .ok_or_else(|| MusicLibraryError::not_found("Music subscription", id))
    }

    pub(super) fn acknowledge(
        &mut self,
        window: &str,
        id: &str,
        sequence: u64,
        now: i64,
    ) -> MusicLibraryResult<()> {
        let subscriber = self.subscriber(window, id)?;
        if sequence == 0 || sequence > subscriber.sequence {
            return Err(MusicLibraryError::validation(
                "sequence",
                "Music acknowledgement was never sent",
            ));
        }
        // Retrying a lost ACK response cannot release a subsequent frame.
        if sequence == subscriber.sequence {
            subscriber.in_flight = None;
        }
        subscriber.expires_at = now + SUBSCRIPTION_LEASE_MS;
        Ok(())
    }

    /// Reading the same frame again is safe after a lost IPC response.
    pub(super) fn read(
        &mut self,
        window: &str,
        id: &str,
        sequence: u64,
        now: i64,
    ) -> MusicLibraryResult<SessionFrame> {
        let subscriber = self.subscriber(window, id)?;
        let retained = subscriber
            .in_flight
            .as_ref()
            .filter(|retained| retained.frame.sequence == sequence)
            .ok_or_else(|| MusicLibraryError::not_found("Music frame", &sequence.to_string()))?;
        let mut frame = retained.frame.clone();
        if let Some(queue) = &retained.queue {
            frame.snapshot.queue = Some(queue.as_ref().clone());
        }
        subscriber.expires_at = now + SUBSCRIPTION_LEASE_MS;
        Ok(frame)
    }

    pub(super) fn unsubscribe(&mut self, window: &str, id: &str) {
        if self
            .subscribers
            .get(window)
            .is_some_and(|subscriber| subscriber.id == id)
        {
            self.subscribers.remove(window);
            if window == self.primary_window {
                self.browser_until = 0;
            }
        }
    }

    pub(super) fn renew_host(
        &mut self,
        window: &str,
        id: &str,
        available: bool,
        now: i64,
    ) -> MusicLibraryResult<bool> {
        self.subscriber(window, id)?.expires_at = now + SUBSCRIPTION_LEASE_MS;
        if window == self.primary_window {
            self.browser_until = if available { now + BROWSER_LEASE_MS } else { 0 };
            if !available {
                self.subscriber(window, id)?.effects.retain(|effect| {
                    matches!(
                        effect,
                        SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
                    )
                });
            }
        }
        Ok(self.browser_available(now))
    }

    pub(super) fn is_host(&self, window: &str, id: &str, now: i64) -> bool {
        window == self.primary_window
            && self.browser_available(now)
            && self
                .subscribers
                .get(window)
                .is_some_and(|subscriber| subscriber.id == id)
    }

    pub(super) fn browser_available(&self, now: i64) -> bool {
        now < self.browser_until && self.subscribers.contains_key(&self.primary_window)
    }

    pub(super) fn expire(&mut self, now: i64) {
        self.subscribers
            .retain(|_, subscriber| now < subscriber.expires_at);
        if !self.browser_available(now) {
            self.browser_until = 0;
            if let Some(subscriber) = self.subscribers.get_mut(&self.primary_window) {
                subscriber.effects.retain(|effect| {
                    matches!(
                        effect,
                        SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
                    )
                });
            }
        }
    }

    /// Coalesces position updates and shares the canonical queue until its revision changes.
    pub(super) fn publish(&mut self, mut snapshot: SessionProjection) {
        if let Some(queue) = snapshot.queue.take() {
            self.queue = Arc::new(queue);
        }
        self.snapshot = Some(snapshot);
        for subscriber in self.subscribers.values_mut() {
            subscriber.dirty = true;
        }
    }

    /// Browser mechanisms have one recipient and a hard pending-work limit.
    pub(super) fn effect(&mut self, effect: SessionEffect, now: i64) -> MusicLibraryResult<()> {
        let stopping = matches!(
            effect,
            SessionEffect::Pause { .. } | SessionEffect::Stop { .. }
        );
        if !stopping && !self.browser_available(now) {
            return Err(MusicLibraryError::conflict(
                "Music browser host is unavailable",
            ));
        }
        let Some(subscriber) = self.subscribers.get_mut(&self.primary_window) else {
            return if stopping {
                Ok(())
            } else {
                Err(MusicLibraryError::conflict(
                    "Music browser host is unavailable",
                ))
            };
        };
        if matches!(effect, SessionEffect::Stop { .. }) {
            subscriber.effects.clear();
        }
        if subscriber.effects.len() >= MAX_PENDING_EFFECTS {
            subscriber.effects.clear();
            self.browser_until = 0;
            return Err(MusicLibraryError::conflict(
                "Music browser host did not consume its bounded effects",
            ));
        }
        subscriber.effects.push(effect);
        Ok(())
    }

    /// Sends at most one unacknowledged frame per window, including while JavaScript is suspended.
    pub(super) fn flush(&mut self, now: i64) {
        let Some(snapshot) = &self.snapshot else {
            return;
        };
        let queue_key = (snapshot.session_id.clone(), snapshot.queue_revision);
        let mut failed = Vec::new();
        for (window, subscriber) in &mut self.subscribers {
            if subscriber.in_flight.is_some()
                || (!subscriber.dirty && subscriber.effects.is_empty())
            {
                continue;
            }
            let projection = snapshot.clone();
            let queue =
                (subscriber.queue_key.as_ref() != Some(&queue_key)).then(|| self.queue.clone());
            subscriber.sequence += 1;
            let frame = SessionFrame {
                subscription_id: subscriber.id.clone(),
                sequence: subscriber.sequence,
                effects_expire_at_ms: now + BROWSER_LEASE_MS,
                snapshot: projection,
                effects: std::mem::take(&mut subscriber.effects),
            };
            let notice = SessionNotice::Update {
                subscription_id: subscriber.id.clone(),
                sequence: subscriber.sequence,
            };
            match subscriber.channel.send(notice) {
                Ok(()) => {
                    subscriber.in_flight = Some(RetainedFrame { frame, queue });
                    subscriber.dirty = false;
                    subscriber.queue_key = Some(queue_key.clone());
                }
                Err(error) => {
                    eprintln!("disconnect failed Music subscription {window}: {error}");
                    failed.push(window.clone());
                }
            }
        }
        for window in failed {
            self.subscribers.remove(&window);
        }
        if !self.browser_available(now) {
            self.browser_until = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::music::session::{models::SessionStatus, policy::SessionPolicy};
    use serde_json::Value;
    use std::sync::Mutex;
    use tauri::ipc::InvokeResponseBody;

    fn channel() -> (Channel<SessionNotice>, Arc<Mutex<Vec<Value>>>) {
        let frames = Arc::new(Mutex::new(Vec::new()));
        let received = frames.clone();
        (
            Channel::new(move |body| {
                let InvokeResponseBody::Json(json) = body else {
                    panic!("expected JSON frame")
                };
                received
                    .lock()
                    .unwrap()
                    .push(serde_json::from_str(&json).unwrap());
                Ok(())
            }),
            frames,
        )
    }

    fn snapshot(revision: u64) -> SessionProjection {
        let mut state = SessionPolicy::new("session".into(), 7);
        state.revision = revision;
        state.projection(true, 100)
    }

    fn subscribed() -> (Subscriptions, Arc<Mutex<Vec<Value>>>) {
        let mut subscriptions = Subscriptions::new("primary".into());
        let (channel, frames) = channel();
        subscriptions
            .subscribe("primary".into(), "subscription".into(), channel, 100)
            .unwrap();
        subscriptions.publish(snapshot(1));
        subscriptions.flush(100);
        (subscriptions, frames)
    }

    #[test]
    fn native_music_stream_coalesces_suspended_snapshots_and_reuses_queue_revision() {
        let (mut subscriptions, frames) = subscribed();
        let first = subscriptions
            .read("primary", "subscription", 1, 100)
            .unwrap();
        for revision in 2..=1_000 {
            let mut snapshot = snapshot(revision);
            snapshot.queue = None;
            subscriptions.publish(snapshot);
            subscriptions.flush(200);
        }
        assert_eq!(frames.lock().unwrap().len(), 1);
        subscriptions
            .acknowledge("primary", "subscription", 1, 300)
            .unwrap();
        subscriptions.flush(300);
        let frames = frames.lock().unwrap();
        assert_eq!(frames.len(), 2);
        let latest = subscriptions
            .read("primary", "subscription", 2, 300)
            .unwrap();
        assert_eq!(latest.snapshot.revision, 1_000);
        assert!(first.snapshot.queue.is_some());
        assert!(latest.snapshot.queue.is_none());
        assert_eq!(frames[0].as_object().unwrap().len(), 3);
    }

    #[test]
    fn native_music_stream_lost_ack_retry_cannot_release_a_later_frame() {
        let (mut subscriptions, frames) = subscribed();
        subscriptions.publish(snapshot(2));
        subscriptions
            .acknowledge("primary", "subscription", 1, 200)
            .unwrap();
        subscriptions.flush(200);
        subscriptions.publish(snapshot(3));
        subscriptions
            .acknowledge("primary", "subscription", 1, 300)
            .unwrap();
        subscriptions.flush(300);
        assert_eq!(frames.lock().unwrap().len(), 2);
        assert!(
            subscriptions
                .acknowledge("primary", "subscription", 3, 300)
                .is_err()
        );
        assert!(
            subscriptions
                .acknowledge("primary", "subscription", 0, 300)
                .is_err()
        );
        subscriptions
            .acknowledge("primary", "subscription", 2, 400)
            .unwrap();
        subscriptions.flush(400);
        assert_eq!(frames.lock().unwrap().len(), 3);
        assert_eq!(
            subscriptions
                .read("primary", "subscription", 3, 400)
                .unwrap()
                .snapshot
                .revision,
            3
        );
    }

    #[test]
    fn native_music_stream_has_one_browser_host_and_window_scoped_acknowledgements() {
        let (mut subscriptions, primary) = subscribed();
        let (channel, secondary) = channel();
        subscriptions
            .subscribe("detached".into(), "second".into(), channel, 100)
            .unwrap();
        assert!(
            !subscriptions
                .renew_host("detached", "second", true, 100)
                .unwrap()
        );
        assert!(
            subscriptions
                .renew_host("primary", "subscription", true, 100)
                .unwrap()
        );
        assert!(subscriptions.is_host("primary", "subscription", 200));
        assert!(!subscriptions.is_host("detached", "second", 200));
        assert!(
            subscriptions
                .acknowledge("detached", "subscription", 1, 200)
                .is_err()
        );
        subscriptions
            .effect(SessionEffect::Play { generation: 1 }, 200)
            .unwrap();
        subscriptions
            .acknowledge("primary", "subscription", 1, 200)
            .unwrap();
        subscriptions.flush(200);
        assert_eq!(primary.lock().unwrap().len(), 2);
        assert_eq!(
            subscriptions
                .read("primary", "subscription", 2, 200)
                .unwrap()
                .effects,
            vec![SessionEffect::Play { generation: 1 }]
        );
        assert_eq!(secondary.lock().unwrap().len(), 1);
        assert!(
            subscriptions
                .read("detached", "second", 1, 200)
                .unwrap()
                .effects
                .is_empty()
        );
    }

    #[test]
    fn native_music_stream_bounds_effects_and_stop_supersedes_obsolete_browser_work() {
        let (mut subscriptions, frames) = subscribed();
        subscriptions
            .renew_host("primary", "subscription", true, 100)
            .unwrap();
        for _ in 0..MAX_PENDING_EFFECTS {
            subscriptions
                .effect(SessionEffect::Play { generation: 1 }, 200)
                .unwrap();
        }
        assert!(
            subscriptions
                .effect(SessionEffect::Play { generation: 1 }, 200)
                .is_err()
        );
        assert!(!subscriptions.browser_available(200));
        subscriptions
            .effect(SessionEffect::Stop { generation: 2 }, 200)
            .unwrap();
        subscriptions
            .acknowledge("primary", "subscription", 1, 200)
            .unwrap();
        subscriptions.flush(200);
        assert_eq!(
            subscriptions
                .read("primary", "subscription", 2, 200)
                .unwrap()
                .effects,
            vec![SessionEffect::Stop { generation: 2 }]
        );
        assert_eq!(frames.lock().unwrap().len(), 2);
    }

    #[test]
    fn native_music_stream_expired_hosts_drop_pending_playback_and_reclaim_capacity() {
        let (mut subscriptions, frames) = subscribed();
        subscriptions
            .renew_host("primary", "subscription", true, 100)
            .unwrap();
        subscriptions
            .effect(SessionEffect::Play { generation: 1 }, 200)
            .unwrap();
        subscriptions.expire(100 + BROWSER_LEASE_MS);
        assert!(!subscriptions.browser_available(100 + BROWSER_LEASE_MS));
        subscriptions
            .acknowledge("primary", "subscription", 1, 6_000)
            .unwrap();
        subscriptions.flush(6_000);
        assert_eq!(frames.lock().unwrap().len(), 1);
        subscriptions.expire(6_000 + SUBSCRIPTION_LEASE_MS);
        assert!(subscriptions.subscribers.is_empty());
        assert!(
            subscriptions
                .renew_host("primary", "subscription", true, 40_000)
                .is_err()
        );
    }

    #[test]
    fn native_music_stream_replaced_subscription_rejects_old_callbacks_and_cleanup() {
        let (mut subscriptions, _) = subscribed();
        let (channel, frames) = channel();
        subscriptions
            .subscribe("primary".into(), "replacement".into(), channel, 200)
            .unwrap();
        subscriptions.unsubscribe("primary", "subscription");
        assert!(
            subscriptions
                .acknowledge("primary", "subscription", 1, 200)
                .is_err()
        );
        subscriptions.flush(200);
        assert_eq!(frames.lock().unwrap()[0]["subscriptionId"], "replacement");
        assert!(
            subscriptions
                .read("primary", "replacement", 1, 200)
                .unwrap()
                .snapshot
                .queue
                .is_some()
        );
    }

    #[test]
    fn native_music_stream_restores_full_queue_for_new_sessions_and_revisions() {
        let (mut subscriptions, frames) = subscribed();
        let mut next = snapshot(2);
        next.session_id = "next-session".into();
        next.status = SessionStatus::Paused;
        subscriptions.publish(next);
        subscriptions
            .acknowledge("primary", "subscription", 1, 200)
            .unwrap();
        subscriptions.flush(200);
        assert!(
            subscriptions
                .read("primary", "subscription", 2, 200)
                .unwrap()
                .snapshot
                .queue
                .is_some()
        );
        let mut next = snapshot(3);
        next.session_id = "next-session".into();
        next.queue_revision += 1;
        subscriptions.publish(next);
        subscriptions
            .acknowledge("primary", "subscription", 2, 300)
            .unwrap();
        subscriptions.flush(300);
        assert_eq!(frames.lock().unwrap().len(), 3);
        assert!(
            subscriptions
                .read("primary", "subscription", 3, 300)
                .unwrap()
                .snapshot
                .queue
                .is_some()
        );
    }

    #[test]
    fn native_music_stream_context_invalidation_revokes_retained_frames_before_next_acknowledgement()
     {
        let (mut subscriptions, frames) = subscribed();
        subscriptions
            .renew_host("primary", "subscription", true, 100)
            .unwrap();
        subscriptions
            .effect(SessionEffect::Play { generation: 1 }, 200)
            .unwrap();
        subscriptions.invalidate_context();
        assert_eq!(
            frames.lock().unwrap()[1],
            serde_json::json!({ "kind": "invalidate", "subscriptionId": "subscription" })
        );
        assert!(
            subscriptions
                .read("primary", "subscription", 1, 300)
                .is_err()
        );
        assert!(
            subscriptions
                .acknowledge("primary", "subscription", 1, 300)
                .is_err()
        );
        assert!(!subscriptions.browser_available(300));
        assert!(subscriptions.snapshot.is_none());
        assert!(subscriptions.queue.is_empty());
        subscriptions.publish(snapshot(2));
        subscriptions.flush(400);
        assert_eq!(frames.lock().unwrap().len(), 2);
    }

    #[test]
    fn native_music_stream_reads_are_retryable_but_cannot_cross_windows_or_acknowledged_frames() {
        let (mut subscriptions, _) = subscribed();
        let first = serde_json::to_value(
            subscriptions
                .read("primary", "subscription", 1, 200)
                .unwrap(),
        )
        .unwrap();
        let retry = serde_json::to_value(
            subscriptions
                .read("primary", "subscription", 1, 300)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(first, retry);
        assert!(subscriptions.read("other", "subscription", 1, 300).is_err());
        assert!(
            subscriptions
                .read("primary", "subscription", 2, 300)
                .is_err()
        );
        subscriptions
            .acknowledge("primary", "subscription", 1, 300)
            .unwrap();
        assert!(
            subscriptions
                .read("primary", "subscription", 1, 300)
                .is_err()
        );
    }

    #[test]
    fn native_music_stream_subscriber_capacity_is_bounded_and_window_replacement_is_allowed() {
        let (mut subscriptions, _) = subscribed();
        for index in 1..MAX_SUBSCRIBERS {
            subscriptions
                .subscribe(
                    format!("window-{index}"),
                    format!("id-{index}"),
                    channel().0,
                    100,
                )
                .unwrap();
        }
        assert!(
            subscriptions
                .subscribe("overflow".into(), "id".into(), channel().0, 100)
                .is_err()
        );
        subscriptions
            .subscribe("primary".into(), "replacement".into(), channel().0, 100)
            .unwrap();
        assert_eq!(subscriptions.subscribers.len(), MAX_SUBSCRIBERS);
        subscriptions.expire(100 + SUBSCRIPTION_LEASE_MS);
        subscriptions
            .subscribe("overflow".into(), "id".into(), channel().0, 40_000)
            .unwrap();
        assert_eq!(subscriptions.subscribers.len(), 1);
    }

    #[test]
    fn native_music_stream_delivery_failure_disconnects_the_host_without_failing_native_publication()
     {
        let mut subscriptions = Subscriptions::new("primary".into());
        let channel = Channel::new(|_| Err(tauri::Error::AssetNotFound("closed".into())));
        subscriptions
            .subscribe("primary".into(), "id".into(), channel, 100)
            .unwrap();
        subscriptions
            .renew_host("primary", "id", true, 100)
            .unwrap();
        subscriptions.publish(snapshot(1));
        subscriptions.flush(100);
        assert!(!subscriptions.browser_available(100));
        assert!(subscriptions.subscribers.is_empty());
    }
}

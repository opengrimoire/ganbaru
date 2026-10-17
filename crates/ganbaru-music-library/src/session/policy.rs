//! Deterministic queue, membership-boundary, and observation policy.

use super::models::*;
use crate::{
    MusicItemAvailability, MusicListeningOutcome, MusicListeningUpdate, MusicRepeatMode,
    MusicSelectionKind, MusicWeight,
};
use std::collections::HashSet;
use std::sync::Arc;

/// Activation failures remain visible without pausing unrelated accepted playback.
#[cfg(desktop)]
#[derive(Clone, Debug)]
pub enum ContextFailure {
    Calendar(String),
    Soundscape(String),
}

#[derive(Clone, Debug, Default)]
pub struct Transition {
    pub effects: Vec<SessionEffect>,
    pub listening: Vec<MusicListeningUpdate>,
    pub changed: bool,
}

#[derive(Clone, Debug)]
pub struct SessionPolicy {
    pub session_id: String,
    pub revision: u64,
    pub generation: u64,
    pub queue_revision: u64,
    pub queue: Arc<Vec<SessionQueueEntry>>,
    pub definition: Option<SessionQueueIntent>,
    pub current: Option<usize>,
    pub playlist_id: Option<String>,
    pub queue_name: String,
    pub owner: SessionOwner,
    pub context: Option<SessionContext>,
    pub status: SessionStatus,
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    pub volume: f64,
    pub muted: bool,
    pub rate: f64,
    selection_volume: Option<f64>,
    selection_rate: Option<f64>,
    pub order: PlaybackOrder,
    pub repeat_mode: MusicRepeatMode,
    pub online: bool,
    pub browser_host: bool,
    pub issue: Option<SessionIssue>,
    pub error: Option<String>,
    #[cfg(desktop)]
    pub context_error: Option<ContextFailure>,
    pub history: Vec<usize>,
    pub recent: Vec<String>,
    pub shuffle: Vec<usize>,
    pub failed: HashSet<usize>,
    pub random_state: u64,
    pub last_sequence: u64,
    pub manual_revision: u64,
    pub soundscape_version: Option<i64>,
    pub autoplay_requested: bool,
    pub suspended: Option<Box<SessionPolicy>>,
    pub review_checkpoint_id: Option<String>,
    selection_open: bool,
    selection_finished: bool,
    selection_kind: MusicSelectionKind,
    last_skip_target: Option<u64>,
}

impl SessionPolicy {
    fn project_error(&self) -> Option<String> {
        #[cfg(desktop)]
        if self.error.is_none()
            && let Some(error) = &self.context_error
        {
            return Some(match error {
                ContextFailure::Calendar(message) | ContextFailure::Soundscape(message) => {
                    message.clone()
                }
            });
        }
        self.error.clone()
    }
    pub fn new(session_id: String, seed: u64) -> Self {
        Self {
            session_id,
            revision: 0,
            generation: 0,
            queue_revision: 0,
            queue: Arc::new(Vec::new()),
            definition: None,
            current: None,
            playlist_id: None,
            queue_name: String::new(),
            owner: SessionOwner::Manual,
            context: None,
            status: SessionStatus::Idle,
            position_ms: 0,
            duration_ms: None,
            volume: 0.8,
            muted: false,
            rate: 1.0,
            selection_volume: None,
            selection_rate: None,
            order: PlaybackOrder::Shuffle,
            repeat_mode: MusicRepeatMode::Off,
            online: true,
            browser_host: false,
            issue: None,
            error: None,
            #[cfg(desktop)]
            context_error: None,
            history: Vec::new(),
            recent: Vec::new(),
            shuffle: Vec::new(),
            failed: HashSet::new(),
            random_state: seed.max(1),
            last_sequence: 0,
            manual_revision: 0,
            soundscape_version: None,
            autoplay_requested: false,
            suspended: None,
            review_checkpoint_id: None,
            selection_open: false,
            selection_finished: false,
            selection_kind: MusicSelectionKind::Manual,
            last_skip_target: None,
        }
    }

    pub fn current_entry(&self) -> Option<&SessionQueueEntry> {
        self.current.and_then(|index| self.queue.get(index))
    }

    pub fn skip_reason(&self, index: usize, explicit: bool, now_ms: i64) -> Option<SkipReason> {
        let Some(entry) = self.queue.get(index) else {
            return Some(SkipReason::InvalidSource);
        };
        if !entry.enabled {
            return Some(SkipReason::Disabled);
        }
        if !entry.phase_allowed {
            return Some(SkipReason::PhaseConstraint);
        }
        if !explicit
            && (entry.snoozed_indefinitely
                || entry.snoozed_until.is_some_and(|until| until > now_ms))
        {
            return Some(SkipReason::Snoozed);
        }
        if !entry.bound {
            return Some(SkipReason::UnboundRoot);
        }
        if entry.source.kind != SourceKind::LocalFile && !self.online {
            return Some(SkipReason::Offline);
        }
        if entry.embedding_blocked {
            return Some(SkipReason::EmbeddingBlocked);
        }
        if entry.availability != MusicItemAvailability::Available || self.failed.contains(&index) {
            return Some(SkipReason::Unavailable);
        }
        None
    }

    pub fn eligible(&self, now_ms: i64) -> Vec<usize> {
        (0..self.queue.len())
            .filter(|index| self.skip_reason(*index, false, now_ms).is_none())
            .collect()
    }

    pub fn projection(&self, include_queue: bool, now_ms: i64) -> SessionProjection {
        let eligible = self.eligible(now_ms);
        let eligible_set: HashSet<_> = eligible.iter().copied().collect();
        let can_next = match self.order {
            PlaybackOrder::Mix => !eligible.is_empty(),
            PlaybackOrder::Shuffle => {
                self.shuffle
                    .iter()
                    .any(|index| eligible_set.contains(index))
                    || (self.repeat_mode != MusicRepeatMode::Off && !eligible.is_empty())
            }
            PlaybackOrder::InOrder => {
                eligible
                    .iter()
                    .any(|index| self.current.is_none_or(|current| *index > current))
                    || (self.repeat_mode != MusicRepeatMode::Off && !eligible.is_empty())
            }
        };
        SessionProjection {
            soundscape_version: self.soundscape_version,
            session_id: self.session_id.clone(),
            revision: self.revision,
            generation: self.generation,
            queue_revision: self.queue_revision,
            current_index: self.current,
            current_source: self.current_entry().map(|entry| entry.source.clone()),
            backend: self.current_entry().map(|entry| entry.backend),
            status: self.status,
            position_ms: self.position_ms,
            duration_ms: self.duration_ms,
            volume: self.selection_volume.unwrap_or(self.volume),
            muted: self.muted,
            rate: self.selection_rate.unwrap_or(self.rate),
            order: self.order,
            repeat_mode: self.repeat_mode,
            owner: self.owner,
            context: self.context.clone(),
            playlist_id: self.playlist_id.clone(),
            queue_name: self.queue_name.clone(),
            can_previous: self
                .history
                .iter()
                .any(|index| eligible_set.contains(index))
                || self
                    .current
                    .is_some_and(|current| eligible.iter().any(|index| *index < current))
                || (self.repeat_mode != MusicRepeatMode::Off && !eligible.is_empty()),
            can_next,
            issue: self.issue,
            error: self.project_error(),
            review_checkpoint_id: self.review_checkpoint_id.clone(),
            queue: include_queue.then(|| self.queue.as_ref().clone()),
        }
    }

    fn draw(&mut self) -> f64 {
        self.random_state ^= self.random_state << 13;
        self.random_state ^= self.random_state >> 7;
        self.random_state ^= self.random_state << 17;
        (self.random_state >> 11) as f64 / ((1_u64 << 53) as f64)
    }

    fn rebuild_shuffle(&mut self, eligible: &[usize], avoid: Option<usize>) {
        self.shuffle = eligible
            .iter()
            .copied()
            .filter(|index| Some(*index) != avoid)
            .collect();
        for index in (1..self.shuffle.len()).rev() {
            let other = (self.draw() * (index + 1) as f64).floor() as usize;
            self.shuffle.swap(index, other);
        }
    }

    pub fn initial(
        &mut self,
        explicit_item: Option<&str>,
        avoid_item: Option<&str>,
        autoplay: bool,
        now_ms: i64,
    ) -> Transition {
        self.autoplay_requested = autoplay;
        let explicit = explicit_item.and_then(|id| {
            self.queue
                .iter()
                .position(|entry| entry.item_id.as_deref().unwrap_or(&entry.source.identity) == id)
        });
        let eligible = self.eligible(now_ms);
        let avoid = avoid_item.and_then(|id| {
            self.queue
                .iter()
                .position(|entry| entry.item_id.as_deref() == Some(id))
        });
        let selected = if let Some(index) =
            explicit.filter(|index| self.skip_reason(*index, true, now_ms).is_none())
        {
            Some(index)
        } else if self.order == PlaybackOrder::Mix {
            if let Some(index) = avoid {
                self.remember(index);
            }
            let draw = self.draw();
            select_mix(&self.queue, &eligible, &self.recent, draw)
        } else if self.order == PlaybackOrder::Shuffle {
            self.rebuild_shuffle(&eligible, avoid);
            self.shuffle
                .first()
                .copied()
                .or_else(|| eligible.first().copied())
        } else {
            eligible
                .iter()
                .copied()
                .find(|index| avoid.is_none_or(|avoid| *index > avoid))
                .or_else(|| eligible.iter().copied().find(|index| Some(*index) != avoid))
                .or_else(|| eligible.first().copied())
        };
        let mut transition = Transition::default();
        if let Some(index) = selected {
            if self.order == PlaybackOrder::Shuffle && explicit.is_some() {
                self.rebuild_shuffle(&eligible, Some(index));
            }
            self.select(
                index,
                autoplay,
                false,
                explicit.is_some(),
                now_ms,
                &mut transition,
            );
        } else {
            self.no_eligible(&mut transition);
        }
        self.finish(transition)
    }

    fn remember(&mut self, index: usize) {
        let identity = &self.queue[index].source.identity;
        self.recent.retain(|prior| prior != identity);
        self.recent.insert(0, identity.clone());
        self.recent.truncate(MAX_RECENT);
    }

    fn select(
        &mut self,
        index: usize,
        autoplay: bool,
        remember_current: bool,
        explicit: bool,
        now_ms: i64,
        transition: &mut Transition,
    ) {
        if self.skip_reason(index, explicit, now_ms).is_some() {
            return;
        }
        if remember_current && let Some(current) = self.current.filter(|current| *current != index)
        {
            self.history.push(current);
            if self.history.len() > MAX_HISTORY {
                self.history.remove(0);
            }
        }
        self.current = Some(index);
        self.selection_volume = self.queue[index].volume;
        self.selection_rate = self.queue[index].rate;
        self.generation += 1;
        self.last_sequence = 0;
        self.position_ms = self.queue[index].source.start_ms.unwrap_or(0);
        self.duration_ms = None;
        self.last_skip_target = None;
        self.selection_open = false;
        self.selection_finished = false;
        self.selection_kind = if explicit {
            MusicSelectionKind::Manual
        } else {
            MusicSelectionKind::Automatic
        };
        self.error = None;
        self.issue = None;
        self.shuffle.retain(|candidate| *candidate != index);
        self.remember(index);
        self.load_effect(autoplay, transition);
    }

    fn load_effect(&mut self, autoplay: bool, transition: &mut Transition) {
        let Some(entry) = self.current_entry() else {
            return;
        };
        let source = entry.source.clone();
        let backend = entry.backend;
        let volume = self.selection_volume.unwrap_or(self.volume);
        let rate = self.selection_rate.unwrap_or(self.rate);
        let autoplay = autoplay && (backend != SessionBackend::Browser || self.browser_host);
        self.autoplay_requested = autoplay;
        if backend == SessionBackend::Browser && !self.browser_host {
            self.status = SessionStatus::Paused;
            self.issue = Some(SessionIssue::BrowserHostUnavailable);
        } else {
            self.status = if autoplay {
                SessionStatus::Loading
            } else {
                SessionStatus::Ready
            };
        }
        transition.effects.push(SessionEffect::Load {
            session_id: self.session_id.clone(),
            generation: self.generation,
            source: Box::new(source),
            backend,
            position_ms: self.position_ms,
            autoplay,
            volume,
            muted: self.muted,
            rate,
        });
        transition.changed = true;
    }

    fn no_eligible(&mut self, transition: &mut Transition) {
        self.status = SessionStatus::Paused;
        self.issue = Some(SessionIssue::NoEligibleItems);
        transition.effects.push(SessionEffect::Pause {
            generation: self.generation,
        });
        transition.changed = true;
    }

    fn outcome(
        &mut self,
        outcome: MusicListeningOutcome,
        now_ms: i64,
        transition: &mut Transition,
    ) {
        if !self.selection_open {
            return;
        }
        if let Some(item_id) = self.current_entry().and_then(|entry| entry.item_id.clone()) {
            transition.listening.push(MusicListeningUpdate {
                playlist_id: self.playlist_id.clone(),
                item_id,
                selection_kind: self.selection_kind,
                outcome,
                occurred_at: now_ms,
            });
        }
        if outcome != MusicListeningOutcome::Started {
            self.selection_open = false;
        }
    }

    fn advance(&mut self, automatic: bool, now_ms: i64, transition: &mut Transition) {
        let eligible = self.eligible(now_ms);
        let next = if automatic
            && self.repeat_mode == MusicRepeatMode::One
            && self.order != PlaybackOrder::Mix
            && self
                .current
                .is_some_and(|current| eligible.contains(&current))
        {
            self.current
        } else {
            match self.order {
                PlaybackOrder::Mix => {
                    let draw = self.draw();
                    select_mix(&self.queue, &eligible, &self.recent, draw)
                }
                PlaybackOrder::Shuffle => {
                    self.shuffle
                        .retain(|index| eligible.contains(index) && Some(*index) != self.current);
                    if self.shuffle.is_empty()
                        && (self.current.is_none() || self.repeat_mode != MusicRepeatMode::Off)
                    {
                        self.rebuild_shuffle(&eligible, self.current);
                    }
                    self.shuffle.first().copied().or_else(|| {
                        (self.repeat_mode != MusicRepeatMode::Off)
                            .then_some(self.current)
                            .flatten()
                            .filter(|index| eligible.contains(index))
                    })
                }
                PlaybackOrder::InOrder => eligible
                    .iter()
                    .copied()
                    .find(|index| self.current.is_none_or(|current| *index > current))
                    .or_else(|| {
                        (self.repeat_mode != MusicRepeatMode::Off)
                            .then(|| eligible.first().copied())
                            .flatten()
                    }),
            }
        };
        if let Some(index) = next {
            // Decoder completion/failure cannot reverse a later Pause or a
            // silently prepared assignment. Explicit navigation still plays.
            let autoplay = !automatic || self.autoplay_requested;
            self.select(index, autoplay, true, false, now_ms, transition);
        } else {
            self.status = if automatic {
                SessionStatus::Ended
            } else {
                SessionStatus::Paused
            };
            if eligible.is_empty() {
                self.issue = Some(SessionIssue::NoEligibleItems);
            }
            transition.effects.push(SessionEffect::Pause {
                generation: self.generation,
            });
            transition.changed = true;
        }
    }

    fn observation(
        &mut self,
        observation: SessionObservation,
        now_ms: i64,
        transition: &mut Transition,
    ) {
        if observation.session_id != self.session_id
            || observation.generation != self.generation
            || observation.sequence <= self.last_sequence
            || self.selection_finished
            || self
                .current_entry()
                .is_none_or(|entry| entry.source.identity != observation.source_identity)
        {
            return;
        }
        self.last_sequence = observation.sequence;
        self.position_ms = observation.position_ms;
        self.duration_ms = observation.duration_ms;
        if observation.status == SessionStatus::Playing && !self.autoplay_requested {
            self.status = SessionStatus::Paused;
            transition.effects.push(SessionEffect::Pause {
                generation: self.generation,
            });
            transition.changed = true;
            return;
        }
        self.status = observation.status;
        self.error = observation.error;
        transition.changed = true;
        if self.status == SessionStatus::Playing && !self.selection_open {
            self.selection_open = true;
            self.outcome(MusicListeningOutcome::Started, now_ms, transition);
        }
        if self.status == SessionStatus::Error {
            self.selection_finished = true;
            self.outcome(MusicListeningOutcome::Skipped, now_ms, transition);
            if let Some(index) = self.current {
                self.failed.insert(index);
            }
            self.issue = Some(SessionIssue::SourceFailure);
            self.advance(true, now_ms, transition);
            return;
        }
        let Some(entry) = self.current_entry() else {
            return;
        };
        let end = entry.source.end_ms;
        let skip = entry
            .skip_ranges
            .iter()
            .find(|range| self.position_ms >= range.start_ms && self.position_ms < range.end_ms)
            .map(|range| range.end_ms);
        if self.status == SessionStatus::Ended
            || (self.status == SessionStatus::Playing
                && end.is_some_and(|end| self.position_ms >= end))
        {
            self.selection_finished = true;
            self.outcome(MusicListeningOutcome::Completed, now_ms, transition);
            self.advance(true, now_ms, transition);
        } else if self.status == SessionStatus::Playing
            && let Some(target) = skip.filter(|target| Some(*target) != self.last_skip_target)
        {
            if end.is_some_and(|end| target >= end) {
                self.selection_finished = true;
                self.outcome(MusicListeningOutcome::Completed, now_ms, transition);
                self.advance(true, now_ms, transition);
            } else {
                self.last_skip_target = Some(target);
                transition.effects.push(SessionEffect::Seek {
                    generation: self.generation,
                    position_ms: target,
                });
            }
        } else if skip.is_none() {
            self.last_skip_target = None;
        }
    }

    pub fn apply(&mut self, intent: SessionIntent, now_ms: i64) -> Transition {
        let mut transition = Transition::default();
        match intent {
            SessionIntent::Observe { observation } => {
                self.observation(observation, now_ms, &mut transition)
            }
            SessionIntent::Next => {
                self.outcome(MusicListeningOutcome::Skipped, now_ms, &mut transition);
                self.advance(false, now_ms, &mut transition);
            }
            SessionIntent::Select { index } => {
                if self.skip_reason(index, true, now_ms).is_none() {
                    self.outcome(MusicListeningOutcome::Skipped, now_ms, &mut transition);
                    self.select(index, true, true, true, now_ms, &mut transition);
                }
            }
            SessionIntent::Previous => {
                let eligible = self.eligible(now_ms);
                let previous = loop {
                    match self.history.pop() {
                        Some(index) if eligible.contains(&index) => break Some(index),
                        Some(_) => continue,
                        None => break None,
                    }
                }
                .or_else(|| {
                    eligible
                        .iter()
                        .rev()
                        .copied()
                        .find(|index| self.current.is_some_and(|current| *index < current))
                })
                .or_else(|| {
                    (self.repeat_mode != MusicRepeatMode::Off)
                        .then(|| eligible.last().copied())
                        .flatten()
                });
                if let Some(index) = previous {
                    self.outcome(MusicListeningOutcome::Skipped, now_ms, &mut transition);
                    self.select(index, true, false, true, now_ms, &mut transition);
                }
            }
            SessionIntent::Play => self.play(now_ms, &mut transition),
            SessionIntent::Toggle => {
                if matches!(self.status, SessionStatus::Playing | SessionStatus::Loading) {
                    self.pause(&mut transition);
                } else {
                    self.play(now_ms, &mut transition);
                }
            }
            SessionIntent::Pause => self.pause(&mut transition),
            SessionIntent::Stop => {
                self.outcome(MusicListeningOutcome::Skipped, now_ms, &mut transition);
                self.generation += 1;
                self.status = SessionStatus::Idle;
                self.position_ms = 0;
                self.current = None;
                transition.effects.push(SessionEffect::Stop {
                    generation: self.generation,
                });
                transition.changed = true;
            }
            SessionIntent::Seek { position_ms } => self.seek(position_ms, &mut transition),
            SessionIntent::SeekBy { delta_ms } => self.seek(
                self.position_ms.saturating_add_signed(delta_ms),
                &mut transition,
            ),
            SessionIntent::Volume { volume } => {
                if volume.is_finite() {
                    self.volume = volume.clamp(0.0, 1.0);
                    self.selection_volume = None;
                    self.settings(&mut transition);
                }
            }
            SessionIntent::Muted { muted } => {
                self.muted = muted;
                self.settings(&mut transition);
            }
            SessionIntent::Rate { rate } => {
                if rate.is_finite() {
                    self.rate = rate.clamp(0.25, 2.0);
                    self.selection_rate = None;
                    self.settings(&mut transition);
                }
            }
            SessionIntent::Order { order } => {
                self.order = order;
                let eligible = self.eligible(now_ms);
                self.rebuild_shuffle(&eligible, self.current);
                transition.changed = true;
            }
            SessionIntent::Online { online } => {
                self.online = online;
                transition.changed = true;
            }
            SessionIntent::BrowserHost { available } => {
                self.browser_host = available;
                if self
                    .current_entry()
                    .is_some_and(|entry| entry.backend == SessionBackend::Browser)
                {
                    if !available {
                        self.pause(&mut transition);
                        self.issue = Some(SessionIssue::BrowserHostUnavailable);
                    } else if self.issue == Some(SessionIssue::BrowserHostUnavailable) {
                        self.issue = None;
                        self.generation += 1;
                        self.last_sequence = 0;
                        self.load_effect(false, &mut transition);
                    }
                }
                transition.changed = true;
            }
            SessionIntent::SuspendReview => {
                if self.owner != SessionOwner::Review {
                    let checkpoint_id = format!("{}:{}", self.session_id, self.revision);
                    self.suspended = Some(Box::new(self.clone()));
                    self.review_checkpoint_id = Some(checkpoint_id);
                    self.owner = SessionOwner::Review;
                    self.pause(&mut transition);
                }
            }
            SessionIntent::RestoreReview { checkpoint_id } => {
                if self.owner == SessionOwner::Review
                    && self.review_checkpoint_id.as_deref() == Some(&checkpoint_id)
                    && let Some(prior) = self.suspended.take()
                {
                    let generation = self.generation + 1;
                    let revision = self.revision;
                    let manual_revision = self.manual_revision;
                    let browser_host = self.browser_host;
                    let autoplay = prior.status == SessionStatus::Playing;
                    *self = *prior;
                    self.generation = generation;
                    self.revision = revision;
                    self.manual_revision = manual_revision;
                    self.browser_host = browser_host;
                    self.last_sequence = 0;
                    self.queue_revision += 1;
                    self.review_checkpoint_id = None;
                    self.load_effect(autoplay, &mut transition);
                }
            }
            SessionIntent::Refresh | SessionIntent::RetryContext => {
                transition.changed = true;
            }
        }
        self.finish(transition)
    }

    fn finish(&mut self, transition: Transition) -> Transition {
        if transition.changed {
            self.revision += 1;
        }
        transition
    }

    /// Retains the selected queue and position while revoking observations from a lost decoder.
    #[cfg(any(not(target_os = "ios"), test))]
    pub fn interrupt_backend(&mut self) -> Transition {
        self.generation += 1;
        self.last_sequence = 0;
        self.autoplay_requested = false;
        self.status = SessionStatus::Paused;
        self.issue = Some(SessionIssue::Interrupted);
        self.error = Some("Native media backend stopped".into());
        self.finish(Transition {
            changed: true,
            ..Transition::default()
        })
    }

    /// Reloads the same selection paused before a separately accepted explicit Play.
    #[cfg(any(not(target_os = "ios"), test))]
    pub fn reconnect_backend(&mut self) -> Transition {
        self.generation += 1;
        self.last_sequence = 0;
        self.error = None;
        self.issue = None;
        let mut transition = Transition::default();
        self.load_effect(false, &mut transition);
        self.finish(transition)
    }

    /// Reconciles canonical rows by durable identity, retaining history across reordering.
    pub fn reconcile_queue(&mut self, prior: &SessionPolicy, now_ms: i64) -> Transition {
        fn key(entry: &SessionQueueEntry) -> &str {
            entry
                .membership_id
                .as_deref()
                .or(entry.item_id.as_deref())
                .unwrap_or(&entry.source.identity)
        }
        let by_id: std::collections::HashMap<_, _> = self
            .queue
            .iter()
            .enumerate()
            .map(|(index, entry)| (key(entry), index))
            .collect();
        let remap = |index: &usize| {
            prior
                .queue
                .get(*index)
                .and_then(|entry| by_id.get(key(entry)))
                .copied()
        };
        self.current = prior.current.as_ref().and_then(remap);
        self.history = prior.history.iter().filter_map(remap).collect();
        self.shuffle = prior.shuffle.iter().filter_map(remap).collect();
        self.failed = prior.failed.iter().filter_map(remap).collect();
        let newly_blocked = self
            .current
            .is_some_and(|index| self.skip_reason(index, false, now_ms).is_some())
            && prior
                .current
                .is_none_or(|index| prior.skip_reason(index, false, now_ms).is_none());
        let mut transition = Transition {
            changed: true,
            ..Transition::default()
        };
        if prior.current.is_some() && (self.current.is_none() || newly_blocked) {
            let mut outgoing = prior.clone();
            outgoing.outcome(MusicListeningOutcome::Skipped, now_ms, &mut transition);
            self.advance(false, now_ms, &mut transition);
            if !prior.autoplay_requested {
                self.pause(&mut transition);
            }
        } else if let (Some(current), Some(previous)) =
            (self.current_entry(), prior.current_entry())
        {
            let changed_path =
                current.source.path != previous.source.path || current.backend != previous.backend;
            if current.volume != previous.volume {
                self.selection_volume = current.volume;
            }
            if self
                .current_entry()
                .is_some_and(|entry| entry.rate != previous.rate)
            {
                self.selection_rate = self.current_entry().and_then(|entry| entry.rate);
            }
            if changed_path {
                self.generation += 1;
                self.last_sequence = 0;
                self.load_effect(prior.autoplay_requested, &mut transition);
            } else {
                self.seek(self.position_ms, &mut transition);
                self.settings(&mut transition);
            }
        }
        self.finish(transition)
    }

    fn pause(&mut self, transition: &mut Transition) {
        self.autoplay_requested = false;
        if self.current.is_none() {
            return;
        }
        self.status = SessionStatus::Paused;
        transition.effects.push(SessionEffect::Pause {
            generation: self.generation,
        });
        transition.changed = true;
    }

    fn play(&mut self, now_ms: i64, transition: &mut Transition) {
        self.autoplay_requested = true;
        if self
            .current_entry()
            .is_some_and(|entry| entry.backend == SessionBackend::Browser)
            && !self.browser_host
        {
            self.issue = Some(SessionIssue::BrowserHostUnavailable);
            self.pause(transition);
            return;
        }
        self.issue = None;
        if matches!(self.status, SessionStatus::Ended | SessionStatus::Error)
            && let Some(index) = self.current
        {
            self.failed.remove(&index);
            self.select(index, true, false, true, now_ms, transition);
            return;
        }
        if self.current.is_some() {
            self.status = SessionStatus::Playing;
            transition.effects.push(SessionEffect::Play {
                generation: self.generation,
            });
            transition.changed = true;
        } else {
            self.advance(false, now_ms, transition);
        }
    }

    fn seek(&mut self, target: u64, transition: &mut Transition) {
        let Some(entry) = self.current_entry() else {
            return;
        };
        let minimum = entry.source.start_ms.unwrap_or(0);
        let maximum = entry.source.end_ms.or(self.duration_ms).unwrap_or(u64::MAX);
        let position_ms = target.max(minimum).min(maximum.max(minimum));
        self.position_ms = position_ms;
        self.last_skip_target = None;
        transition.effects.push(SessionEffect::Seek {
            generation: self.generation,
            position_ms,
        });
        transition.changed = true;
    }

    fn settings(&mut self, transition: &mut Transition) {
        transition.effects.push(self.settings_effect());
        transition.changed = true;
    }

    /// Returns current effective gain and rate without mutating persisted user settings.
    pub fn settings_effect(&self) -> SessionEffect {
        SessionEffect::Settings {
            generation: self.generation,
            volume: self.selection_volume.unwrap_or(self.volume),
            muted: self.muted,
            rate: self.selection_rate.unwrap_or(self.rate),
        }
    }
}

/// Selects from fixed user weights with a soft penalty for five distinct recent tracks.
pub fn select_mix(
    queue: &[SessionQueueEntry],
    eligible: &[usize],
    recent: &[String],
    draw: f64,
) -> Option<usize> {
    const RECENT_MULTIPLIERS: [f64; MAX_RECENT] = [0.05, 0.2, 0.4, 0.65, 0.85];
    let candidates: Vec<_> = eligible
        .iter()
        .filter_map(|index| {
            let entry = queue.get(*index)?;
            let weight = match entry.weight {
                MusicWeight::Rarely => 0.5,
                MusicWeight::LessOften => 0.75,
                MusicWeight::Normal => 1.0,
                MusicWeight::MoreOften => 1.5,
                MusicWeight::MuchMoreOften => 2.0,
            };
            let recent_weight = recent
                .iter()
                .position(|identity| identity == &entry.source.identity)
                .and_then(|rank| RECENT_MULTIPLIERS.get(rank))
                .copied()
                .unwrap_or(1.0);
            Some((*index, weight * recent_weight))
        })
        .collect();
    let mut remainder = draw.clamp(0.0, 1.0 - f64::EPSILON)
        * candidates.iter().map(|(_, weight)| weight).sum::<f64>();
    for (index, weight) in &candidates {
        if remainder < *weight {
            return Some(*index);
        }
        remainder -= weight;
    }
    candidates.last().map(|(index, _)| *index)
}

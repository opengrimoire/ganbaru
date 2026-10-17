//! Temporary completion attenuation belongs to the serialized Music owner, never user settings.

use super::*;
use crate::sound_effects::{AppSound, AppSoundState};
use std::time::Instant;

const COMPLETION_ATTENUATION: f64 = 0.0;
const COMPLETION_MAX_LIFETIME: Duration = Duration::from_secs(30);

pub(super) struct CompletionDuck {
    session_id: String,
    generation: u64,
    manual_revision: u64,
    focus_identity: Option<String>,
    expires_at: Instant,
    response: oneshot::Receiver<Result<(), String>>,
}

impl CompletionDuck {
    pub fn matches(&self, state: &SessionPolicy) -> bool {
        self.session_id == state.session_id
            && self.generation == state.generation
            && self.manual_revision == state.manual_revision
    }

    fn may_resume(&self, state: &SessionPolicy, focus_identity: Option<String>) -> bool {
        self.matches(state)
            && self.focus_identity == focus_identity
            && state.status == SessionStatus::Paused
            && state.issue.is_none()
    }

    fn finished(&mut self, now: Instant) -> Option<Result<(), String>> {
        match self.response.try_recv() {
            Ok(result) => Some(result),
            Err(oneshot::error::TryRecvError::Closed) => Some(Err(
                "Focus completion audio worker dropped its result".into(),
            )),
            Err(oneshot::error::TryRecvError::Empty) if now >= self.expires_at => Some(Err(
                "Focus completion attenuation exceeded its deadline".into(),
            )),
            Err(oneshot::error::TryRecvError::Empty) => None,
        }
    }
}

impl Owner {
    pub(super) async fn begin_completion(
        &mut self,
        scope: (u64, i64),
        sound: AppSound,
    ) -> MusicLibraryResult<()> {
        if !crate::pomodoro::native_runtime::presentation_is_current(&self.app, scope.0, scope.1) {
            return Err(MusicLibraryError::conflict(
                "Focus completion was superseded",
            ));
        }
        self.release_completion().await?;
        let expires_at = Instant::now() + COMPLETION_MAX_LIFETIME;
        let should_duck = matches!(
            self.state.status,
            SessionStatus::Playing | SessionStatus::Loading
        ) && !self.state.muted
            && self.state.current_entry().is_some();
        if should_duck {
            let (reply, response) = oneshot::channel();
            // Retain restoration before the first backend call. A lost or partially
            // applied response must not leave gain dependent on successful audio setup.
            drop(reply);
            self.completion_duck = Some(CompletionDuck {
                session_id: self.state.session_id.clone(),
                generation: self.state.generation,
                manual_revision: self.state.manual_revision,
                focus_identity: self
                    .focus
                    .as_ref()
                    .map(focus::FocusLease::completion_identity),
                expires_at,
                response,
            });
            if let Err(error) = self.apply_effect(self.state.settings_effect()).await {
                // Restore even if a backend reported failure after partially applying gain.
                if let Err(restore) = self.release_completion().await {
                    return Err(MusicLibraryError::runtime(
                        "attenuate completion Music",
                        format!("{error}; restore gain: {restore}"),
                    ));
                }
                return Err(MusicLibraryError::runtime(
                    "attenuate completion Music",
                    error,
                ));
            }
            let mut paused = self.state.clone();
            let transition = paused.apply(SessionIntent::Pause, now_ms());
            if let Err(error) = self.commit(paused, transition, None, true).await {
                if let Err(restore) = self.release_completion().await {
                    return Err(MusicLibraryError::runtime(
                        "pause completion Music",
                        format!("{error}; restore gain: {restore}"),
                    ));
                }
                return Err(error);
            }
        }
        let queued = self.app.state::<AppSoundState>().play_focus_completion(
            sound,
            self.app.clone(),
            scope,
            expires_at,
        );
        match queued {
            Ok(response) if should_duck => {
                self.completion_duck
                    .as_mut()
                    .expect("attenuation retains its restoration")
                    .response = response;
            }
            Ok(_) => {}
            Err(error) => {
                if should_duck {
                    self.release_completion().await.map_err(|restore| {
                        MusicLibraryError::runtime(
                            "enqueue completion audio",
                            format!("{error}; restore gain: {restore}"),
                        )
                    })?;
                }
                return Err(MusicLibraryError::runtime(
                    "enqueue completion audio",
                    error,
                ));
            }
        }
        Ok(())
    }

    pub(super) fn attenuate_completion(&self, mut effect: SessionEffect) -> SessionEffect {
        if self
            .completion_duck
            .as_ref()
            .is_some_and(|duck| duck.matches(&self.state))
        {
            attenuate(&mut effect);
        }
        effect
    }

    pub(super) async fn release_completion(&mut self) -> MusicLibraryResult<()> {
        let Some(duck) = self.completion_duck.take() else {
            return Ok(());
        };
        let should_resume = duck.may_resume(
            &self.state,
            self.focus
                .as_ref()
                .map(focus::FocusLease::completion_identity),
        );
        if let Err(error) = self.apply_effect(self.state.settings_effect()).await {
            // Keep restoration eligible after an uncertain backend response.
            self.completion_duck = Some(duck);
            return Err(MusicLibraryError::runtime(
                "restore completion Music gain",
                error,
            ));
        }
        if should_resume {
            let mut resumed = self.state.clone();
            let transition = resumed.apply(SessionIntent::Play, now_ms());
            // Install can itself release a superseded token; box this bounded
            // re-entry rather than creating an unbounded recursive future type.
            if let Err(error) = Box::pin(self.commit(resumed, transition, None, true)).await {
                self.completion_duck = Some(duck);
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) async fn poll_completion(&mut self) -> MusicLibraryResult<()> {
        let result = self
            .completion_duck
            .as_mut()
            .and_then(|duck| duck.finished(Instant::now()));
        if let Some(result) = result {
            if let Err(error) = result {
                eprintln!("Native Focus completion audio: {error}");
            }
            self.release_completion().await?;
        }
        Ok(())
    }
}

fn attenuate(effect: &mut SessionEffect) {
    if let SessionEffect::Settings { volume, .. } | SessionEffect::Load { volume, .. } = effect {
        *volume *= COMPLETION_ATTENUATION;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn duck(
        state: &SessionPolicy,
        now: Instant,
    ) -> (CompletionDuck, oneshot::Sender<Result<(), String>>) {
        let (reply, response) = oneshot::channel();
        (
            CompletionDuck {
                session_id: state.session_id.clone(),
                generation: state.generation,
                manual_revision: state.manual_revision,
                focus_identity: None,
                expires_at: now + COMPLETION_MAX_LIFETIME,
                response,
            },
            reply,
        )
    }

    #[test]
    fn completion_gain_never_changes_canonical_volume_mute_or_rate() {
        let mut state = SessionPolicy::new("music".into(), 1);
        state.volume = 0.73;
        state.muted = true;
        state.rate = 1.25;
        let original = state.settings_effect();
        let mut effect = original.clone();
        attenuate(&mut effect);
        assert!(matches!(
            effect,
            SessionEffect::Settings {
                volume: 0.0,
                muted: true,
                rate: 1.25,
                ..
            }
        ));
        assert!(matches!(
            state.settings_effect(),
            SessionEffect::Settings {
                volume: 0.73,
                muted: true,
                rate: 1.25,
                ..
            }
        ));
        let mut pause = SessionEffect::Pause { generation: 7 };
        attenuate(&mut pause);
        assert!(matches!(pause, SessionEffect::Pause { generation: 7 }));
    }

    #[test]
    fn completion_tokens_cannot_own_a_manual_action_replacement_track_or_new_session() {
        let state = SessionPolicy::new("music".into(), 1);
        let (lease, _reply) = duck(&state, Instant::now());
        assert!(lease.matches(&state));
        for changed in ["manual", "generation", "session"] {
            let mut newer = state.clone();
            match changed {
                "manual" => newer.manual_revision += 1,
                "generation" => newer.generation += 1,
                _ => newer.session_id = "another".into(),
            }
            assert!(!lease.matches(&newer), "{changed}");
        }
    }

    #[test]
    fn completion_resume_requires_its_own_pause_and_unchanged_focus_context() {
        let mut state = SessionPolicy::new("music".into(), 1);
        state.status = SessionStatus::Paused;
        let (mut lease, _reply) = duck(&state, Instant::now());
        lease.focus_identity = Some("accepted-phase".into());
        assert!(lease.may_resume(&state, Some("accepted-phase".into())));
        assert!(!lease.may_resume(&state, Some("later-phase".into())));
        assert!(!lease.may_resume(&state, None));
        let mut manual_pause = state.clone();
        manual_pause.manual_revision += 1;
        assert!(!lease.may_resume(&manual_pause, Some("accepted-phase".into())));
        for status in [
            SessionStatus::Idle,
            SessionStatus::Playing,
            SessionStatus::Error,
            SessionStatus::Ended,
        ] {
            let mut changed = state.clone();
            changed.status = status;
            assert!(!lease.may_resume(&changed, Some("accepted-phase".into())));
        }
        state.issue = Some(SessionIssue::PersistenceFailure);
        assert!(!lease.may_resume(&state, Some("accepted-phase".into())));
    }

    #[test]
    fn completion_releases_gain_for_audio_success_failure_lost_reply_and_stall() {
        let state = SessionPolicy::new("music".into(), 1);
        let now = Instant::now();
        for result in [Ok(()), Err("output failed".to_string())] {
            let (mut lease, reply) = duck(&state, now);
            assert!(lease.finished(now).is_none());
            reply.send(result.clone()).unwrap();
            assert_eq!(lease.finished(now), Some(result));
        }
        let (mut lost, reply) = duck(&state, now);
        drop(reply);
        assert!(lost.finished(now).unwrap().unwrap_err().contains("dropped"));
        let (mut stalled, _reply) = duck(&state, now);
        assert!(
            stalled
                .finished(now + COMPLETION_MAX_LIFETIME - Duration::from_nanos(1))
                .is_none()
        );
        assert!(
            stalled
                .finished(now + COMPLETION_MAX_LIFETIME)
                .unwrap()
                .unwrap_err()
                .contains("deadline")
        );
    }
}
